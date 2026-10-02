//! Sending a message through the router. The one message model is `prov::Message`: a request
//! to a worker, a worker's or a computer-use run's progress and final report, the person's own
//! turn to a subagent and a note across Spaces are all that one type. docket adds no second
//! one. What lives here is the *draft* a sender hands the router to stamp, and what the router
//! answers; the stamped result is a `prov::Message`.
//!
//! **A message carries no authority.** A `Request` is input: the receiver evaluates it under
//! its own `TaskPolicy` and the full gating pipeline, exactly as if it had thought of it
//! itself. The message's label joins into the receiver's taint, so a request that came from
//! something that read mail leaves the receiver tainted. A message may cross Spaces and
//! carries its labels, but never grants a memory read in the other Space.

use crate::context::Reveal;
use crate::ids::Handle;
use prov::{
    Address, AgentRef, Crossing, EntityId, Fault, MessageId, MessageKind, MessageText, OutcomeRef,
    ThreadId, UndoHandle,
};
use serde::{Deserialize, Serialize};

/// One piece of a draft. Text is typed; a handle is a value the router holds, whose label folds
/// into the message's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum DraftPart {
    /// Words the sender typed.
    Text(MessageText),
    /// A value the router resolves, with its label.
    Handle(Handle),
    /// A thing an app owns.
    Entity(EntityId),
    /// The outcome of an action.
    Outcome(OutcomeRef),
    /// An undo journal entry.
    Undo(UndoHandle),
}

/// What a sender hands the router. Everything else about the message (id, sender, label,
/// time) is stamped by the router from the transport and the session, never taken from here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageDraft {
    /// Who it is for, and in which Space.
    pub to: Address,
    /// The thread it continues; none starts one.
    pub thread: Option<ThreadId>,
    /// The message it answers.
    pub in_reply_to: Option<MessageId>,
    /// What it is.
    pub kind: MessageKind,
    /// What it says.
    pub parts: Vec<DraftPart>,
}

/// The router's answer to a send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delivery {
    /// The message as stamped.
    pub message: MessageId,
    /// Its thread.
    pub thread: ThreadId,
    /// Whether it crossed Spaces.
    pub crossing: Crossing,
}

/// Why a message was not delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum SendRefusal {
    /// The draft is malformed.
    Malformed(Fault),
    /// The sender may not speak for that party.
    SenderMismatch,
    /// Nobody is there to receive it.
    NoRecipient,
    /// A handle the session does not hold.
    UnknownHandle,
    /// The Space is halted or the session is paused.
    Halted,
}

/// One piece of a message that landed for a task, as its planner may read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum InboundPart {
    /// Words: plain when the label is trusted, a handle when not.
    Text(Reveal<String>),
    /// A thing.
    Entity(EntityId),
    /// An outcome.
    Outcome(OutcomeRef),
    /// An undo entry.
    Undo(UndoHandle),
}

/// A message that landed for a task. It is input only; nothing in it widens the task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboundLine {
    /// The message.
    pub id: MessageId,
    /// Its thread.
    pub thread: ThreadId,
    /// Who sent it, in which Space.
    pub from: Address,
    /// Whether it crossed Spaces: it shows on the roster either way.
    pub crossing: Crossing,
    /// What it is.
    pub kind: MessageKind,
    /// What it says.
    pub parts: Vec<InboundPart>,
}

/// Which agent's inbox to read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxAsk {
    /// The agent that reads.
    pub agent: AgentRef,
    /// Messages after this one (none: all that wait).
    pub after: Option<MessageId>,
}
