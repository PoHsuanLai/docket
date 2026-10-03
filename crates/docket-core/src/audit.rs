//! What the router appends to the event log. Never content: ids, kinds, decisions, codes.
//! intentd stores most of these as `EventBody::Area { area: Docket }` in this serde form, with
//! the things each names for cascade-forget; a message and an episode are almanac's own typed
//! bodies, so they travel as such.

use crate::budget::HaltCause;
use crate::call::CallEnd;
use crate::confirm::{ConfirmEnd, GrantScope};
use crate::ids::{ActionRef, CallId, UndoId};
use crate::review::{BreakerTrip, PolicyId, ReasonCode, ReviewMark, Stage};
use crate::task_policy::{PolicyChange, TaskPolicyState};
use crate::undo::UndoState;
use almanac_core::Episode;
use prov::{
    Actor, AgentRef, ConfirmId, ConfirmReceipt, Effect, EntityId, InputProof, Message, SessionId,
    SpaceId, SpaceScope, TaskId, UnixSeconds,
};
use serde::{Deserialize, Serialize};

/// Who decided a call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum DecidedBy {
    /// Policy alone, by these rules.
    Policy(Vec<PolicyId>),
    /// A reviewer tightened or passed it.
    Reviewer {
        /// The coded reason.
        code: ReasonCode,
        /// The last stage that ran.
        stage: Stage,
    },
    /// The person, by this receipt.
    User(ConfirmReceipt),
}

/// How a confirmation was answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ConfirmAnswerKind {
    /// Yes, once or always.
    Allowed(GrantScope),
    /// Yes, and the terminal may run this action until logout.
    AllowedFromTerminal,
    /// It ended without a yes.
    Ended(ConfirmEnd),
}

/// How a policy changed, without its contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyChangeKind {
    /// Covers less.
    Narrows,
    /// Covers the same.
    Same,
    /// Covers more.
    Widens,
}

impl PolicyChange {
    /// The change without its detail.
    pub fn kind(&self) -> PolicyChangeKind {
        match self {
            PolicyChange::Narrows => PolicyChangeKind::Narrows,
            PolicyChange::Same => PolicyChangeKind::Same,
            PolicyChange::Widens(_) => PolicyChangeKind::Widens,
        }
    }
}

/// One thing the router records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum AuditRecord {
    /// A call ended.
    Call {
        /// When.
        at: UnixSeconds,
        /// Which.
        call: CallId,
        /// Who.
        actor: Actor,
        /// The action.
        action: ActionRef,
        /// What it touched.
        targets: Vec<EntityId>,
        /// Its effect.
        effect: Effect,
        /// The Space.
        space: SpaceId,
        /// Who decided.
        decided: DecidedBy,
        /// How it ended.
        end: CallEnd,
    },
    /// One stage of a review ended; every stage is logged once.
    Review {
        /// When.
        at: UnixSeconds,
        /// Which call.
        call: CallId,
        /// What happened.
        mark: ReviewMark,
    },
    /// A confirmation was answered.
    Confirm {
        /// When.
        at: UnixSeconds,
        /// Which.
        id: ConfirmId,
        /// How.
        answer: ConfirmAnswerKind,
        /// What the answer came from, if a yes.
        input: Option<InputProof>,
    },
    /// An undo ended.
    Undo {
        /// When.
        at: UnixSeconds,
        /// Which entry.
        entry: UndoId,
        /// Who undid it.
        by: Actor,
        /// How it ended.
        end: UndoState,
    },
    /// A halt began or ended.
    Halt {
        /// When.
        at: UnixSeconds,
        /// Which Spaces.
        scope: SpaceScope,
        /// Why.
        cause: HaltCause,
    },
    /// A task policy was made, narrowed, widened, lapsed or revoked.
    TaskPolicy {
        /// When.
        at: UnixSeconds,
        /// The task.
        task: TaskId,
        /// Where it stands.
        state: TaskPolicyState,
        /// How it changed.
        change: PolicyChangeKind,
    },
    /// The breaker tripped.
    Breaker {
        /// When.
        at: UnixSeconds,
        /// The session.
        session: SessionId,
        /// Why.
        trip: BreakerTrip,
    },
    /// A task was spawned by a step of another.
    TaskStarted {
        /// When.
        at: UnixSeconds,
        /// The new task.
        task: TaskId,
        /// Its agent.
        agent: AgentRef,
        /// The task that spawned it.
        parent: Option<TaskId>,
        /// Its Space.
        space: SpaceId,
        /// The spawning call.
        by_call: CallId,
    },
    /// A message was delivered. Stored as almanac's `EventBody::Message`.
    Message(Box<Message>),
    /// A task ended and left its skeleton, or the idle pass added its narrative. Stored as
    /// almanac's `EventBody::Episode`.
    Episode(Box<Episode>),
    /// A record of the companion's own session (`Session.Note`): stored as
    /// `Area { Companion }` of kind `companion.session.<slug>`, trusted and private to its Space.
    Session {
        /// When.
        at: UnixSeconds,
        /// The Space of the session.
        space: SpaceId,
        /// The record's kind.
        slug: crate::wire::NoteSlug,
        /// The record, in its owner's serde form.
        json: almanac_core::JsonText,
    },
}
