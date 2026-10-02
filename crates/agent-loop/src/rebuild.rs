//! Restart: companiond keeps no state of its own. It rebuilds the roster and the front task
//! from what the eventlog holds: its own session records, the router's task and message
//! events, the computer-use runs' states and the episodes. Events come oldest first.

use crate::front::{FrontEvent, front_step};
use almanac_core::{Episode, EpisodeOutcome};
use companion_wire::{AnswerPhase, SessionRecord};
use docket_core::{LeadText, Roster, RosterDetail, RosterFull, RosterLine, RosterState};
use prov::{
    AgentRef, Message, MessageKind, Part, ReportStatus, RunId, SpaceId, TaskId, UnixSeconds,
};
use serde::{Deserialize, Serialize};

/// What a stored event says, as the rebuild reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ReplayWhat {
    /// A session record of companiond's.
    Session(Box<SessionRecord>),
    /// A message the router delivered.
    Message(Box<Message>),
    /// A task or side episode.
    Episode(Box<Episode>),
    /// A computer-use run's state, mapped from `Area{Cua}` by the caller.
    Run {
        /// The run.
        run: RunId,
        /// Its Space.
        space: SpaceId,
        /// Where it stands.
        state: RosterState,
    },
}

/// One stored event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayEvent {
    /// When it happened.
    pub at: UnixSeconds,
    /// What it says.
    pub what: ReplayWhat,
}

/// One agent as the rebuild found it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RebuiltTask {
    /// Which agent.
    pub agent: AgentRef,
    /// The task, for a front or worker task.
    pub task: Option<TaskId>,
    /// Its Space.
    pub space: SpaceId,
    /// The task that spawned it.
    pub parent: Option<TaskId>,
    /// Where it stands.
    pub state: RosterState,
    /// What the person last told it directly.
    pub told: Option<LeadText>,
}

/// What a restart recovers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rebuilt {
    /// The task the launcher returns to; companiond opens a fresh session for it.
    pub front: Option<TaskId>,
    /// Every agent seen, oldest first.
    pub tasks: Vec<RebuiltTask>,
}

impl Rebuilt {
    /// The roster: only agents still working or waiting, goals unknown after a restart (the
    /// goal text is not in the digest) and so shown as presence only.
    pub fn roster(&self) -> Roster {
        let live = |s: RosterState| {
            matches!(
                s,
                RosterState::Starting
                    | RosterState::Working
                    | RosterState::NeedsYou
                    | RosterState::Paused
            )
        };
        Roster {
            entries: self
                .tasks
                .iter()
                .filter(|t| live(t.state))
                .map(|t| RosterLine {
                    agent: t.agent.clone(),
                    space: t.space.clone(),
                    state: t.state,
                    detail: match &t.told {
                        Some(told) => RosterDetail::Full(Box::new(RosterFull {
                            goal: docket_core::Reveal::Plain(String::new()),
                            last: None,
                            told: Some(told.clone()),
                        })),
                        None => RosterDetail::PresenceOnly,
                    },
                })
                .collect(),
        }
    }
}

fn state_of_phase(phase: &AnswerPhase) -> RosterState {
    match phase {
        AnswerPhase::Thinking | AnswerPhase::Streaming => RosterState::Working,
        AnswerPhase::NeedsYou(_) => RosterState::NeedsYou,
        AnswerPhase::Done => RosterState::Done,
        AnswerPhase::Failed => RosterState::Failed,
        AnswerPhase::Cancelled => RosterState::Cancelled,
    }
}

fn state_of_report(status: ReportStatus) -> RosterState {
    match status {
        ReportStatus::Done => RosterState::Done,
        ReportStatus::Failed => RosterState::Failed,
        ReportStatus::Cancelled => RosterState::Cancelled,
        ReportStatus::Progress => RosterState::Working,
    }
}

fn state_of_outcome(outcome: &EpisodeOutcome) -> RosterState {
    match outcome {
        EpisodeOutcome::Done | EpisodeOutcome::Handed { .. } => RosterState::Done,
        EpisodeOutcome::Failed => RosterState::Failed,
        EpisodeOutcome::Cancelled => RosterState::Cancelled,
        EpisodeOutcome::Open => RosterState::Working,
    }
}

fn entry<'a>(
    tasks: &'a mut Vec<RebuiltTask>,
    agent: &AgentRef,
    space: &SpaceId,
) -> &'a mut RebuiltTask {
    let at = tasks.iter().position(|t| &t.agent == agent);
    let at = at.unwrap_or_else(|| {
        tasks.push(RebuiltTask {
            agent: agent.clone(),
            task: None,
            space: space.clone(),
            parent: None,
            state: RosterState::Starting,
            told: None,
        });
        tasks.len() - 1
    });
    &mut tasks[at]
}

/// Folds the stored events, oldest first, into the agents that exist, where each stands, what
/// the person last told it, and which task is the front.
pub fn rebuild(events: &[ReplayEvent]) -> Rebuilt {
    let mut out = Rebuilt::default();
    for event in events {
        match &event.what {
            ReplayWhat::Session(record) => apply_session(&mut out, record),
            ReplayWhat::Message(m) => {
                let agent = if m.from.agent == AgentRef::User {
                    &m.to
                } else {
                    &m.from
                };
                let t = entry(&mut out.tasks, &agent.agent, &agent.space);
                match m.kind {
                    MessageKind::Report { status } => t.state = state_of_report(status),
                    MessageKind::Request if m.from.agent == AgentRef::User => {
                        let words = m.parts.iter().find_map(|p| match p {
                            Part::Text(text) => Some(text.as_str()),
                            Part::Entity(_) | Part::Outcome(_) | Part::Undo(_) => None,
                        });
                        t.told = words.map(LeadText::of);
                    }
                    MessageKind::Request | MessageKind::Note => {}
                }
            }
            ReplayWhat::Episode(e) => {
                let t = entry(&mut out.tasks, &e.agent, &e.space);
                if e.kind == almanac_core::EpisodeKind::Task {
                    t.state = state_of_outcome(&e.outcome);
                }
            }
            ReplayWhat::Run { run, space, state } => {
                entry(&mut out.tasks, &AgentRef::Cua { run: run.clone() }, space).state = *state;
            }
        }
    }
    out
}

fn apply_session(out: &mut Rebuilt, record: &SessionRecord) {
    match record {
        SessionRecord::Opened {
            task,
            space,
            agent,
            parent,
        } => {
            let t = entry(&mut out.tasks, agent, space);
            t.task = Some(task.clone());
            t.parent = parent.clone();
            t.state = RosterState::Working;
            if *agent == AgentRef::Companion {
                out.front = front_step(out.front.take(), &FrontEvent::Asked(task.clone()));
            }
        }
        SessionRecord::Asked { to, task, turn, .. } => {
            if let Some(t) = out.tasks.iter_mut().find(|t| t.task.as_ref() == Some(task)) {
                t.state = RosterState::Working;
                if *to != AgentRef::Companion {
                    t.told = Some(LeadText::of(&turn.text));
                }
            }
        }
        SessionRecord::Replied { task, phase, .. } | SessionRecord::Finished { task, phase } => {
            let state = state_of_phase(phase);
            if let Some(t) = out.tasks.iter_mut().find(|t| t.task.as_ref() == Some(task)) {
                t.state = state;
            }
            if state != RosterState::Working && state != RosterState::NeedsYou {
                out.front = front_step(out.front.take(), &FrontEvent::Ended(task.clone()));
            }
        }
        SessionRecord::Closed => {}
    }
}
