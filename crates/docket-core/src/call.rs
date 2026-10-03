//! Calls: what a caller asks the router for, what the app is told, and every way a call can end.

use crate::budget::BudgetKind;
use crate::confirm::{ConfirmEnd, ConfirmId};
use crate::context::EntityRef;
use crate::ids::ParamName;
use crate::ids::{ActionRef, CallId, UndoToken};
use crate::preview::Preview;
use crate::review::{BreakerTrip, DenyCode};
use crate::value::{Args, TargetValue, Value};
use porter_core::{AppName, Permille};
use prov::{Actor, EntityId, Labelled, SpaceId, SpaceScope};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Where a call came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// The launcher.
    Launcher,
    /// A prompt field inside an app's window.
    InWindowField,
    /// The companion.
    Companion,
    /// A keyboard shortcut.
    Shortcut,
    /// An external MCP client.
    Mcp,
    /// A process running `quire-do`.
    Cli,
    /// An app calling itself.
    AppInternal,
}

/// What a caller asks the router for (the app is in `action`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallRequest {
    /// Which action.
    pub action: ActionRef,
    /// What it acts on.
    pub target: TargetValue,
    /// Its arguments, with provenance.
    pub args: Args,
    /// Where the call came from.
    pub origin: Origin,
}

/// What the router sends to `IntentProvider1.Perform`. The app sees the actor and the origin
/// (to label its undo entry), never the caller's bus identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invocation {
    /// The call.
    pub call: CallId,
    /// The action, by name (the app is the receiver).
    pub action: prov::ActionName,
    /// What it acts on.
    pub target: TargetValue,
    /// The arguments, handles resolved.
    pub args: Args,
    /// Who acts.
    pub actor: Actor,
    /// Where the call came from.
    pub origin: Origin,
    /// The Space it acts in.
    pub space: SpaceId,
}

/// What a finished action returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    /// A value, labelled.
    pub value: Option<Labelled<Value>>,
    /// "Archived 3 threads", in the app's words.
    pub said: Option<crate::ids::LabelText>,
    /// What to show.
    pub show: Preview,
    /// Whether it can be taken back.
    pub undo: Undoable,
    /// What to do next.
    pub follow: Follow,
}

/// Whether an outcome can be taken back, and with what.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Undoable {
    /// No.
    No,
    /// Yes, with this token.
    Yes(UndoToken),
}

/// What an outcome asks for next; the router caps chains at depth four.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Follow {
    /// Nothing.
    Nothing,
    /// Open this thing.
    Open(EntityId),
    /// Make this call, which is gated afresh.
    Next(CallRequest),
}

/// Words an app wrote about a failure; they may quote the person's content.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FailText(pub String);

impl fmt::Debug for FailText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FailText(<{} bytes>)", self.0.len())
    }
}

/// The app's own refusals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum AppRefusal {
    /// A parameter is missing; here are options.
    NeedsParam {
        /// Which.
        param: ParamName,
        /// What the app suggests.
        options: Vec<EntityRef>,
    },
    /// The thing is gone.
    NotFound(EntityId),
    /// The thing changed since it was named.
    Stale(EntityId),
    /// The app is busy.
    Busy,
    /// The app cannot do that.
    Unsupported,
    /// It failed.
    Failed(FailText),
}

/// Why an argument was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgFault {
    /// Required and absent.
    Missing,
    /// The wrong type.
    WrongType,
    /// Outside its range.
    OutOfRange,
    /// A handle the session does not hold.
    UnknownHandle,
    /// A thing from another Space.
    CrossSpace,
}

/// Everything a caller can get back instead of an [`Outcome`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum CallRefusal {
    /// The app said no.
    App(AppRefusal),
    /// Policy, consent or a reviewer said no; the code is coarse on purpose (no oracle).
    Denied(DenyCode),
    /// The person said no, dismissed it, or it expired.
    Unconfirmed(ConfirmEnd),
    /// The Space is halted.
    Halted(SpaceScope),
    /// The session is paused by the breaker until the person speaks.
    Paused(BreakerTrip),
    /// A budget ran out.
    OverBudget(BudgetKind),
    /// No such action.
    NoSuchAction(ActionRef),
    /// An argument is wrong.
    BadArgs {
        /// Which.
        param: ParamName,
        /// Why.
        why: ArgFault,
    },
    /// The app is not running or not reachable.
    AppUnavailable(AppName),
    /// The app did not answer in time.
    Timeout,
}

/// Where a call is, for the requester's progress signal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum CallProgress {
    /// Being reviewed.
    Reviewing,
    /// Asking the app to describe the change.
    Previewing,
    /// Waiting for the person.
    Confirming(ConfirmId),
    /// Handed to the app.
    Dispatched,
    /// Running, this far along.
    Running(Permille),
}

/// How a call ended, for the audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum CallEnd {
    /// It ran.
    Done,
    /// It did not.
    Refused(CallRefusal),
}
