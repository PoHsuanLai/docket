//! Every reply of `org.quire.Intents1`.

use crate::bodies::{
    Displayed, GrantAnswer, RecallView, Resolved, StoredView, UndoReport, WidenAnswer, WireRefusal,
};
use crate::task::SessionOpened;
use companion_model::{WatchedAgent, WatchedLine, Watching};
use docket_core::{
    CallRefusal, CheckpointFault, CheckpointList, ContextView, Delivery, HandleCard, Hit,
    InboundLine, KillSwitch, Outcome, Preview, RestorePlan, Reveal, UndoEntry, UndoFault,
};
use docket_gate_wire::GateAnswer;
use docket_permissions_core::TaskPolicy;
use docket_vocab::{ActionRef, EntityRef, TurnId, Value};
use prov::SessionId;
use serde::{Deserialize, Serialize};

/// Every reply of `org.quire.Intents1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
#[non_exhaustive]
pub enum IntentsReply {
    /// Nothing to return.
    Done,
    /// The installed manifests.
    Manifests(Vec<docket_manifest::ValidManifest>),
    /// Search hits.
    Hits(Vec<Hit>),
    /// How a call ended.
    Performed(Box<Result<Outcome, CallRefusal>>),
    /// A preview.
    Preview(Preview),
    /// Suggested things.
    Suggestions(Vec<EntityRef>),
    /// An undo of one entry.
    Undone(Result<(), UndoFault>),
    /// An undo of several.
    UndoneAll(UndoReport),
    /// What the person is doing, as a planner may read it.
    Context(Box<ContextView>),
    /// A session was opened.
    SessionOpened(SessionOpened),
    /// A turn was recorded.
    TurnRecorded(TurnId),
    /// A handle's text for the screen (`Session.Display`).
    Text(String),
    /// A handle's text and its label, for the quarantined reader (`Session.Resolve`).
    Resolved(Resolved),
    /// A handle's text and its label, for the screen (`Session.DisplayLabelled`).
    Displayed(Displayed),
    /// What the reader answered: a plain closed-set value, or a handle.
    Read(Reveal<Value>),
    /// The task policy, if there is one.
    TaskPolicy(Option<Box<TaskPolicy>>),
    /// A widening ended.
    Widened(WidenAnswer),
    /// What memory returned.
    Recalled(RecallView),
    /// What a session holds by handle: shape, source and size, never the content.
    Handles(Vec<HandleCard>),
    /// What the durable log of sessions holds.
    Stored(StoredView),
    /// A session's restore points.
    Checkpoints(Box<CheckpointList>),
    /// What restoring a point would change, or why that cannot be said.
    CheckpointPlan(Result<RestorePlan, CheckpointFault>),
    /// A watch session was opened (`Checkpoint.Watch`).
    CheckpointWatching(SessionId),
    /// A terminal named the agent app it watches (`Checkpoint.WatchAgent`): the watch session
    /// to mark turns on, which is the companion's own when the conversation was handed over.
    CheckpointWatchingAgent(Watching),
    /// A session's conversation moved (`Checkpoint.Handover`); the app and its own id for the
    /// conversation, when the router has them.
    HandedOver(Option<WatchedAgent>),
    /// The watch sessions whose agent app a terminal named (`Checkpoint.Watched`).
    Watched(Vec<WatchedLine>),
    /// A message was delivered.
    Delivered(Delivery),
    /// Messages that wait.
    Inbox(Vec<InboundLine>),
    /// A grant request ended.
    Granted(GrantAnswer),
    /// The gate's answer.
    Gate(GateAnswer),
    /// The kill switch.
    State(KillSwitch),
    /// The journal.
    Journal(Vec<UndoEntry>),
    /// The actions the terminal holds a standing grant for.
    TerminalGrants(Vec<ActionRef>),
    /// The standing grants held.
    StandingGrants(Vec<docket_permissions_core::StandingGrant>),
    /// The request was refused.
    Refused(WireRefusal),
}
