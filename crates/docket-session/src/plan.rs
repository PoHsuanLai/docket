//! What `resume_plan` says: the typed plan a host restores a session from, and the faults it
//! found on the way. The plan holds only what the log held: the person's words, the policy as
//! stored, step lines, handle labels. It never holds a value to re-read.

use crate::codec::Unreadable;
use crate::entry::{CallOpen, EndCause, HandleLabel, Opening, Seq, Taint};
use docket_core::{
    BreakerTrip, CallId, Ledger, SkillId, SkillVersion, StepLine, TaskPolicy, UserTurn,
};
use porter_core::Count;
use serde::{Deserialize, Serialize};

/// Why a session takes no new turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Blocker {
    /// Entries are missing between two that exist.
    Gap,
    /// An entry could not be read.
    Unreadable,
}

/// Where the session stands after the fold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Standing {
    /// Taking turns.
    Open,
    /// The breaker tripped: only a new turn resumes it.
    Paused(BreakerTrip),
    /// Over: shown for display; the person reopens it as a fork.
    Closed(EndCause),
    /// The log cannot be trusted to be whole: shown for display, takes no turn.
    Blocked(Blocker),
}

/// A call the crash cut off: it ends interrupted and is never run again. Whether an undoable
/// effect happened is for the journal to say; a destructive one with no journal row is reported
/// as "unknown whether it ran".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interrupted {
    /// The call.
    pub call: CallId,
    /// What it was.
    pub open: CallOpen,
}

/// What a session has used, as far as the log knows. The wall budget counts active time: the
/// host restarts its clock at the resume, from this checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumedBudget {
    /// The last checkpoint, if any was written.
    pub checkpoint: Option<Ledger>,
    /// Calls begun since it (all of them when there is none).
    pub calls_since: Count,
}

/// Something wrong with the log that the fold worked around, failing closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ResumeFault {
    /// Entries `expected..found` are missing.
    Gap {
        /// The position that should have come next.
        expected: Seq,
        /// The one that did.
        found: Seq,
    },
    /// An entry could not be read.
    Unreadable {
        /// Where.
        at: Seq,
        /// Why.
        why: Unreadable,
    },
    /// An untrusted handle with no taint written before it: the session is Tainted anyway.
    MissingTaint {
        /// The handle's entry.
        at: Seq,
    },
    /// A second `Opened`; it was ignored.
    DuplicateOpened {
        /// Where.
        at: Seq,
    },
}

/// Why there is no plan at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PlanRefusal {
    /// No entries.
    #[error("the session has no entries")]
    Empty,
    /// The first entry is not `Opened` (swept by retention, or the log is not this session's).
    #[error("the session's first entry is not its opening")]
    NoOpening,
}

/// The plan: what to restore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumePlan {
    /// How it began.
    pub opening: Opening,
    /// Where it stands.
    pub standing: Standing,
    /// Never lower than any taint in the log, and Tainted when the log is not whole.
    pub taint: Taint,
    /// The task policy as last stored; the policy writer is not called.
    pub policy: Option<TaskPolicy>,
    /// The person's words, in order.
    pub turns: Vec<UserTurn>,
    /// Calls that ended, as the planner's history.
    pub history: Vec<StepLine>,
    /// Calls that did not: ended interrupted, never re-run.
    pub interrupted: Vec<Interrupted>,
    /// Handles as labels only, by handle.
    pub handles: Vec<HandleLabel>,
    /// What it has used.
    pub budget: ResumedBudget,
    /// The skills it loaded; a version mismatch against the installed one warns.
    pub skills: Vec<(SkillId, SkillVersion)>,
    /// Legacy records skipped (`Replied`, `Finished`).
    pub legacy_skipped: Count,
    /// What was wrong with the log.
    pub faults: Vec<ResumeFault>,
}
