//! The records companiond stores about its sessions: one per fact, so the front task and the
//! roster can be rebuilt after a restart, and so the person's turns survive a session. They go
//! to the eventlog as `EventBody::Area { area: Companion }` in this serde form, with
//! `Actor::Companion { role: Planner }` or the person as the actor.

use crate::answer::AnswerPhase;
use almanac_core::Skeleton;
use docket_core::UserTurn;
use prov::{AgentRef, SpaceId, TaskId};
use serde::{Deserialize, Serialize};

/// The kind-tag prefix of these records in the eventlog: `companion.session.<slug>`.
pub const SESSION_KIND_PREFIX: &str = "companion.session";

/// One thing that happened to a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum SessionRecord {
    /// A session opened for an agent, perhaps spawned by another task.
    Opened {
        /// The task it runs.
        task: TaskId,
        /// Its Space.
        space: SpaceId,
        /// The agent it is for.
        agent: AgentRef,
        /// The task that spawned it.
        parent: Option<TaskId>,
    },
    /// The person (or a field of an app) said something to an agent. The turn is stored
    /// verbatim: the person's own words are trusted events, kept for the retention window of
    /// `companion.*` (30 days).
    Asked {
        /// What was said, with its id and where it was recorded.
        turn: UserTurn,
        /// Who it was said to.
        to: AgentRef,
        /// The task it belongs to.
        task: TaskId,
    },
    /// An answer ended in a phase; the digest is the trusted skeleton of what it did, so a
    /// restart need not replay the router's whole audit.
    Replied {
        /// The task.
        task: TaskId,
        /// How it ended.
        phase: AnswerPhase,
        /// What it did, typed.
        digest: Skeleton,
    },
    /// A task finished.
    Finished {
        /// The task.
        task: TaskId,
        /// How it ended.
        phase: AnswerPhase,
    },
    /// The session closed.
    Closed,
}

impl SessionRecord {
    /// The record's slug, the tail of its kind tag.
    pub fn slug(&self) -> &'static str {
        match self {
            SessionRecord::Opened { .. } => "opened",
            SessionRecord::Asked { .. } => "asked",
            SessionRecord::Replied { .. } => "replied",
            SessionRecord::Finished { .. } => "finished",
            SessionRecord::Closed => "closed",
        }
    }
}
