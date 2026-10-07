//! A session rebuilt from its `ResumePlan`: pure, so every rule of a restore is a table test.
//!
//! What comes back as it was: the person's words, the policy as stored (the policy writer is
//! not asked), the history, the taint (never lower than the log's), the breaker's trip. What
//! comes back changed: handles are labels with no value, an interrupted call is a history line
//! that says so and is not run again, the budget's wall clock starts again, nothing is known
//! to have been shown (`known` is empty), and a session whose log cannot be trusted is closed
//! for display.

use crate::handles::HandleTable;
use crate::opening::actor_of;
use crate::session::{CloseCause, SessionState, Taint};
use crate::state::SessionRecord;
use crate::tasks::{TaskRecord, TaskState};
use crate::wal::Wal;
use almanac_core::EpisodeId;
use docket_core::{Ledger, Reveal, Saw, StepEnd, StepLine, StepShown, TaskLedger, UserTurn};
use docket_session::{
    ResumeFault, ResumePlan, SessionEntry, Standing, Taint as Written, TaintCause, TaintNote,
};
use porter_core::{AppName, Count};
use prov::{AgentRef, Label, SessionId, UnixSeconds};

/// The app a session is attributed to when its log never said who opened it (a record from
/// before the durable log).
const FALLBACK_OPENER: &str = crate::companion::COMPANION_APP;

/// A restored session and everything beside it the router must make consistent.
#[derive(Debug)]
pub(crate) struct Rebuilt {
    /// The session.
    pub(crate) record: SessionRecord,
    /// Its task.
    pub(crate) task: TaskRecord,
    /// The highest number any id of it used: new ids must be above it.
    pub(crate) reserve: u64,
    /// The taint the log itself holds: below the session's when a repair is still to be
    /// appended.
    pub(crate) durable: Written,
}

/// Whether the plan found untrusted handles with no taint entry before them.
fn needs_repair(plan: &ResumePlan) -> bool {
    plan.faults
        .iter()
        .any(|f| matches!(f, ResumeFault::MissingTaint { .. }))
}

/// The number in an id of the form `<letter>-<number>`.
pub(crate) fn number_of(id: &str) -> u64 {
    id.rsplit('-')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// The history line of a call a crash cut off.
fn interrupted_line(open: &docket_session::CallOpen) -> StepLine {
    StepLine {
        call: open.call,
        action: open.action.clone(),
        effect: open.effect,
        end: StepEnd::Interrupted,
        shown: StepShown::Full,
        with: Vec::new(),
    }
}

/// Where the session stands, and what its task is doing.
fn state_of(standing: &Standing, taint: Taint) -> (SessionState, TaskState) {
    match standing {
        Standing::Open => (SessionState::Open(taint), TaskState::Working),
        Standing::Paused(trip) => (
            SessionState::Paused { taint, trip: *trip },
            TaskState::NeedsYou,
        ),
        Standing::Closed(cause) => (
            SessionState::Closed((*cause).into()),
            TaskState::Ended(prov::ReportStatus::Done),
        ),
        // A log that cannot be trusted whole is shown, not continued.
        Standing::Blocked(_) => (
            SessionState::Closed(CloseCause::Closed),
            TaskState::Ended(prov::ReportStatus::Done),
        ),
    }
}

/// What the session has used: the last checkpoint plus the calls since, on a wall clock that
/// starts at `now` (budgets count active time, owner decision D5).
fn ledger_of(plan: &ResumePlan, now: UnixSeconds) -> Ledger {
    let base = plan.budget.checkpoint.unwrap_or_else(|| Ledger::new(now));
    Ledger {
        started: now,
        window_start: now,
        in_window: Count(0),
        calls: Count(base.calls.0.saturating_add(plan.budget.calls_since.0)),
        ..base
    }
}

/// The distinct labels of the handles, which a message the session sends joins.
fn labels_of(plan: &ResumePlan) -> Vec<Label> {
    let mut seen: Vec<Label> = Vec::new();
    for handle in &plan.handles {
        if !seen.contains(&handle.label) {
            seen.push(handle.label.clone());
        }
    }
    seen
}

/// What waits to be appended once the session is live: a step for each interrupted call (so
/// the log says what the restore told the planner), and the taint a repair found due.
fn queued(plan: &ResumePlan, lines: &[StepLine]) -> Vec<SessionEntry> {
    let taint = needs_repair(plan).then_some(SessionEntry::Taint(TaintNote {
        cause: TaintCause::Repaired,
        at_call: None,
    }));
    taint
        .into_iter()
        .chain(lines.iter().cloned().map(SessionEntry::Step))
        .collect()
}

/// The session `plan` describes, as of `now`.
pub(crate) fn rebuild(id: &SessionId, plan: &ResumePlan, now: UnixSeconds) -> Rebuilt {
    let opening = &plan.opening;
    let agent = opening.agent.clone().unwrap_or(AgentRef::Companion);
    let opener = opening
        .opener
        .clone()
        .unwrap_or_else(|| AppName::parse(FALLBACK_OPENER).expect("a fixed app name is valid"));
    let taint: Taint = plan.taint.into();
    let (state, task_state) = state_of(&plan.standing, taint);
    let cut_off: Vec<StepLine> = plan
        .interrupted
        .iter()
        .map(|i| interrupted_line(&i.open))
        .collect();

    let mut record = SessionRecord::new(
        opening.task.clone(),
        actor_of(&agent, id, &opener),
        opener,
        opening.space.clone(),
        now,
    );
    record.state = state;
    record.ledger = ledger_of(plan, now);
    record.turns = plan.turns.clone();
    record.policy = plan.policy.clone();
    record.history = plan.history.iter().chain(&cut_off).cloned().collect();
    record.seen = labels_of(plan);
    record.saw.untrusted = match taint {
        Taint::Tainted => Saw::Seen,
        Taint::Clean => Saw::NotSeen,
    };
    // The log does not say whether private text was shown; a session that did anything is
    // taken to have seen some.
    record.saw.private = match record.history.is_empty() && plan.handles.is_empty() {
        true => Saw::NotSeen,
        false => Saw::Seen,
    };
    let mut handles = HandleTable::new();
    plan.handles
        .iter()
        .cloned()
        .for_each(|label| handles.restore_label(label));
    record.handles = handles;
    for (skill, _) in &plan.skills {
        // The cap and the byte budget are not charged again; the body is not held.
        let _ = record.skill_loads.admit(skill, 0);
    }
    record.wal = match &plan.standing {
        Standing::Open | Standing::Paused(_) => {
            let mut wal = Wal::on(plan.taint);
            queued(plan, &cut_off)
                .into_iter()
                .for_each(|entry| wal.note(entry));
            wal
        }
        Standing::Closed(_) | Standing::Blocked(_) => Wal::Off,
    };

    let first = plan.turns.first().map(|t| t.text.clone());
    let ids = plan
        .turns
        .iter()
        .map(|t: &UserTurn| t.id.0)
        .chain(record.history.iter().map(|s| s.call.0))
        .chain([number_of(id.as_str()), number_of(opening.task.as_str())]);
    let reserve = ids.max().unwrap_or(0);
    let task = TaskRecord {
        task: opening.task.clone(),
        session: id.clone(),
        agent: agent.clone(),
        parent: opening.parent.clone(),
        space: opening.space.clone(),
        state: task_state,
        goal: Reveal::Plain(first.unwrap_or_default()),
        last: record.history.last().cloned(),
        ledger: TaskLedger {
            task: opening.task.clone(),
            agent,
            parent: opening
                .parent
                .as_ref()
                .and_then(|p| EpisodeId::parse(p.as_str()).ok()),
            space: opening.space.clone(),
            started: now,
            asked: plan.turns.clone(),
            steps: Vec::new(),
            touched: Vec::new(),
            results: Vec::new(),
        },
    };
    let durable = match needs_repair(plan) {
        true => Written::Clean,
        false => plan.taint,
    };
    Rebuilt {
        record,
        task,
        reserve,
        durable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docket_core::{BreakerTrip, CallId};
    use docket_session::{
        BackendKind, CallOpen, EndCause, Interrupted, Opening, ResumedBudget, TaintCause,
    };
    use prov::{ActionName, Effect, SpaceId, TaskId};

    fn plan(standing: Standing) -> ResumePlan {
        ResumePlan {
            opening: Opening {
                task: TaskId::parse("t-4").expect("task"),
                space: SpaceId::parse("work").expect("space"),
                opener: Some(AppName::parse("org.quire.Shell").expect("app")),
                agent: Some(AgentRef::Companion),
                backend: BackendKind::Native,
                parent: None,
                forked_from: None,
            },
            standing,
            taint: Written::Clean,
            policy: None,
            turns: vec![],
            history: vec![],
            interrupted: vec![],
            handles: vec![],
            budget: ResumedBudget {
                checkpoint: None,
                calls_since: Count(0),
            },
            skills: vec![],
            legacy_skipped: Count(0),
            faults: vec![],
        }
    }

    fn id() -> SessionId {
        SessionId::parse("s-4").expect("session")
    }

    #[test]
    fn the_wall_clock_restarts_and_the_calls_carry_over() {
        let mut p = plan(Standing::Open);
        let then = Ledger {
            calls: Count(7),
            writes: Count(3),
            ..Ledger::new(UnixSeconds(100))
        };
        p.budget = ResumedBudget {
            checkpoint: Some(then),
            calls_since: Count(2),
        };
        let rebuilt = rebuild(&id(), &p, UnixSeconds(5000));
        let ledger = rebuilt.record.ledger;
        assert_eq!(
            ledger.started,
            UnixSeconds(5000),
            "active time, not elapsed"
        );
        assert_eq!(ledger.window_start, UnixSeconds(5000));
        assert_eq!((ledger.calls, ledger.writes), (Count(9), Count(3)));
    }

    #[test]
    fn a_paused_session_restores_paused_and_waits_for_the_person() {
        let trip = BreakerTrip::Consecutive;
        let rebuilt = rebuild(&id(), &plan(Standing::Paused(trip)), UnixSeconds(1));
        assert_eq!(
            rebuilt.record.state,
            SessionState::Paused {
                taint: Taint::Clean,
                trip
            }
        );
        assert_eq!(rebuilt.task.state, TaskState::NeedsYou);
        assert!(rebuilt.record.wal.is_on());
    }

    #[test]
    fn a_closed_or_blocked_session_is_shown_and_never_written() {
        let closed = rebuild(
            &id(),
            &plan(Standing::Closed(EndCause::SpaceHalted)),
            UnixSeconds(1),
        );
        assert_eq!(
            closed.record.state,
            SessionState::Closed(CloseCause::SpaceHalted)
        );
        let mut p = plan(Standing::Blocked(docket_session::Blocker::Gap));
        p.taint = Written::Tainted;
        let blocked = rebuild(&id(), &p, UnixSeconds(1));
        assert_eq!(
            blocked.record.state,
            SessionState::Closed(CloseCause::Closed)
        );
        assert_eq!(blocked.record.saw.untrusted, Saw::Seen);
        for rebuilt in [closed, blocked] {
            assert_eq!(rebuilt.record.wal, Wal::Off);
        }
    }

    #[test]
    fn an_interrupted_call_is_a_history_line_and_a_step_to_write_once() {
        let mut p = plan(Standing::Open);
        let open = CallOpen {
            call: CallId(12),
            action: docket_core::ActionRef {
                app: AppName::parse("org.quire.Mail").expect("app"),
                name: ActionName::parse("mail.thread.archive").expect("action"),
            },
            effect: Effect::UndoableWrite,
        };
        p.interrupted = vec![Interrupted {
            call: CallId(12),
            open,
        }];
        let rebuilt = rebuild(&id(), &p, UnixSeconds(1));
        assert_eq!(rebuilt.record.history.len(), 1);
        assert_eq!(rebuilt.record.history[0].end, StepEnd::Interrupted);
        assert!(rebuilt.reserve >= 12, "new ids start above the call's");
        let Wal::On { pending, .. } = &rebuilt.record.wal else {
            panic!("a live session is on the record")
        };
        assert_eq!(pending.len(), 1);
        assert!(matches!(&pending[0], SessionEntry::Step(l) if l.call == CallId(12)));
    }

    #[test]
    fn untrusted_handles_with_no_taint_are_repaired_on_the_record() {
        let mut p = plan(Standing::Open);
        p.taint = Written::Tainted;
        p.faults = vec![ResumeFault::MissingTaint {
            at: docket_session::Seq(1),
        }];
        let rebuilt = rebuild(&id(), &p, UnixSeconds(1));
        assert_eq!(
            rebuilt.durable,
            Written::Clean,
            "the repair is still to be written"
        );
        let Wal::On { pending, .. } = &rebuilt.record.wal else {
            panic!("on the record")
        };
        assert!(matches!(
            &pending[0],
            SessionEntry::Taint(n) if n.cause == TaintCause::Repaired
        ));
    }
}
