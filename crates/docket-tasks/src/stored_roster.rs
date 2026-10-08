//! The roster and the front task from the sessions the router stores. companiond keeps no state
//! of its own; after a restart it asks the router for the sessions it may bring back
//! (`Session.Stored`) and folds each into the events the rebuild reads: an opening, the person's
//! turns, and the legacy `Replied` / `Finished` notes a log may still hold.
//!
//! A session an editor opened (its opening records a directory) is not the companion's: it never
//! reaches the roster or the front pointer, whatever else the log says about it.

use agent_loop::{ReplayEvent, ReplayWhat};
use companion_wire::SessionRecord;
use docket_session::{Logged, Opening, Read, SessionEntry, SessionLog, read_all};
use prov::{AgentRef, TaskId, UnixSeconds};

use crate::recover::ReplayFault;

/// Whether `row` is the opening of a session an editor started.
fn is_editors(row: &Logged) -> bool {
    matches!(&row.read, Read::Entry(entry)
        if matches!(entry.as_ref(), SessionEntry::Opened(Opening { cwd: Some(_), .. })))
}

/// The events one session's rows tell, in log order. A turn is the only row with a time, so the
/// opening takes the time of the first turn and the rows without one the time of the last.
pub fn events_of(rows: &[Logged]) -> Vec<ReplayEvent> {
    if rows.iter().any(is_editors) {
        return Vec::new();
    }
    let mut at = rows
        .iter()
        .find_map(|row| match &row.read {
            Read::Entry(entry) => match entry.as_ref() {
                SessionEntry::Turn(turn) => Some(turn.at),
                _ => None,
            },
            _ => None,
        })
        .unwrap_or(UnixSeconds(0));
    let mut agent = AgentRef::Companion;
    let mut task: Option<TaskId> = None;
    let mut events = Vec::new();
    for row in rows {
        let what = match &row.read {
            Read::Entry(entry) => match entry.as_ref() {
                SessionEntry::Opened(opening) => {
                    agent = opening.agent.clone().unwrap_or(AgentRef::Companion);
                    task = Some(opening.task.clone());
                    Some(SessionRecord::Opened {
                        task: opening.task.clone(),
                        space: opening.space.clone(),
                        agent: agent.clone(),
                        parent: opening.parent.clone(),
                    })
                }
                SessionEntry::Turn(turn) => {
                    at = turn.at;
                    task.clone().map(|task| SessionRecord::Asked {
                        turn: turn.clone(),
                        to: agent.clone(),
                        task,
                    })
                }
                _ => None,
            },
            Read::Legacy(record) => Some(record.as_ref().clone()),
            Read::Unreadable(_) => None,
        };
        if let Some(record) = what {
            events.push(ReplayEvent {
                at,
                what: ReplayWhat::Session(Box::new(record)),
            });
        }
    }
    events
}

/// The events of every session `log` lists, oldest session first; `None` when it lists none (a
/// router with no session log, or a log that has just begun). A session whose rows cannot be
/// read adds nothing; a log that cannot list is unavailable.
pub async fn stored_events<L: SessionLog>(
    log: &L,
) -> Result<Option<Vec<ReplayEvent>>, ReplayFault> {
    let sessions = log.sessions().await.map_err(|_| ReplayFault::Unavailable)?;
    if sessions.is_empty() {
        return Ok(None);
    }
    let mut events = Vec::new();
    for session in sessions {
        if let Ok(rows) = read_all(log, &session).await {
            events.extend(events_of(&rows));
        }
    }
    Ok(Some(events))
}

#[cfg(test)]
mod tests {
    use super::*;
    use companion_wire::AnswerPhase;
    use docket_core::{TurnId, TurnSource, TurnVia, UserTurn, Workspace};
    use docket_session::{BackendKind, Seq};
    use prov::SpaceId;

    fn opening(task: &str, cwd: Option<&str>) -> SessionEntry {
        SessionEntry::Opened(Opening {
            task: TaskId::parse(task).expect("task"),
            space: SpaceId::parse("work").expect("space"),
            opener: None,
            agent: Some(AgentRef::Companion),
            backend: BackendKind::Native,
            parent: None,
            forked_from: None,
            cwd: cwd.map(|c| Workspace::parse(c).expect("workspace")),
            started_from: None,
        })
    }

    fn turn(at: i64) -> SessionEntry {
        SessionEntry::Turn(UserTurn {
            id: TurnId(1),
            text: "hello".into(),
            at: UnixSeconds(at),
            from: TurnSource::Launcher,
            via: TurnVia::Typed,
        })
    }

    fn rows(reads: Vec<Read>) -> Vec<Logged> {
        reads
            .into_iter()
            .enumerate()
            .map(|(n, read)| Logged {
                seq: Seq(n as u64),
                read,
            })
            .collect()
    }

    fn entry(entry: SessionEntry) -> Read {
        Read::Entry(Box::new(entry))
    }

    #[test]
    fn a_session_is_an_opening_its_turns_and_what_legacy_notes_say_of_its_end() {
        let finished = SessionRecord::Finished {
            task: TaskId::parse("t-1").expect("task"),
            phase: AnswerPhase::Done,
        };
        let events = events_of(&rows(vec![
            entry(opening("t-1", None)),
            entry(turn(50)),
            Read::Legacy(Box::new(finished)),
        ]));
        let rebuilt = agent_loop::rebuild(&events);
        assert_eq!(rebuilt.front, None, "the task finished");
        assert_eq!(rebuilt.tasks.len(), 1);
        assert_eq!(rebuilt.tasks[0].state, docket_core::RosterState::Done);
        // The opening has no time of its own: it takes its first turn's.
        assert_eq!(
            events.iter().map(|e| e.at.0).collect::<Vec<_>>(),
            [50, 50, 50]
        );
    }

    #[test]
    fn an_editors_session_adds_nothing_whatever_else_its_rows_say() {
        // A legacy opening (no directory) comes first and the native one, with it, after.
        let events = events_of(&rows(vec![
            entry(opening("t-2", None)),
            entry(turn(60)),
            entry(opening("t-2", Some("/work/project"))),
        ]));
        assert_eq!(events, Vec::new());
    }
}
