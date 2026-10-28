//! What `resume_plan` says: the typed plan a host restores a session from, and the faults it
//! found on the way. The plan holds only what the log held: the person's words, the policy as
//! stored, step lines, handle labels. It never holds a value to re-read.

use crate::codec::Unreadable;
use crate::entry::{CallOpen, EndCause, HandleLabel, Opening, Seq, Taint};
use companion_model::HandNote;
use companion_model::helper::HelperSeat;
use docket_core::{Ledger, SkillId, SkillVersion, StepLine, Summary, UserTurn};
use docket_permissions_core::{BreakerTrip, TaskPolicy};
use docket_vocab::CallId;
use porter_ids::Count;
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
    /// A summary that covers more than the log held before it, or less than an earlier one did;
    /// it was ignored.
    BadCompaction {
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
    /// The helper seat the session was given, when it has one.
    pub seat: Option<HelperSeat>,
    /// Where the conversation is, when the log moved it or named the agent a terminal watches.
    pub hand: Option<HandNote>,
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
    /// The latest summary of the oldest turns and steps, if the session was compacted. `turns`
    /// and `history` stay whole; [`ResumePlan::live_turns`] and [`ResumePlan::live_history`] are
    /// what the planner is shown beside it.
    pub summary: Option<Summary>,
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

impl ResumePlan {
    /// The person's turns the summary does not stand for: all of them when there is none.
    pub fn live_turns(&self) -> &[UserTurn] {
        let from = self
            .summary
            .as_ref()
            .map_or(0, |s| s.covers.turns.0 as usize);
        self.turns.get(from..).unwrap_or_default()
    }

    /// The steps the summary does not stand for: all of them when there is none.
    pub fn live_history(&self) -> &[StepLine] {
        let from = self
            .summary
            .as_ref()
            .map_or(0, |s| s.covers.steps.0 as usize);
        self.history.get(from..).unwrap_or_default()
    }
}
