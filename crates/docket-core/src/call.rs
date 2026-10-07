//! Calls: what a caller asks the router for, what the app is told, and every way a call can end.

use crate::budget::BudgetKind;
use crate::confirm::{ConfirmEnd, ConfirmId};
use crate::context::EntityRef;
use crate::ids::ParamName;
use crate::ids::{ActionRef, CallId, Handle, UndoId, UndoToken};
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

impl CallRequest {
    /// The handles the call names: its target first, then its arguments in order (a handle
    /// inside a list or a record counts). A request the router has already resolved names none.
    pub fn handles(&self) -> Vec<Handle> {
        fn walk(value: &Value, into: &mut Vec<Handle>) {
            match value {
                Value::Handle(h) => into.push(*h),
                Value::List(items) => items.iter().for_each(|i| walk(i, into)),
                Value::Record(fields) => fields.values().for_each(|v| walk(v, into)),
                _ => {}
            }
        }
        let mut found = match &self.target {
            TargetValue::Handles(hs) => hs.clone(),
            _ => Vec::new(),
        };
        self.args.values().for_each(|a| walk(&a.value, &mut found));
        found
    }
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

/// What `IntentProvider1.Perform` carries: the [`Invocation`] and, beside it, the `classified`
/// effect the router gated on (per-call actions only) and the activation
/// token the launcher sent with the call (a capability the app may redeem with the compositor to
/// take focus). The JSON is the invocation's with a top-level `"activation"` when there is a
/// token, so a provider that reads only an `Invocation` is unaffected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivatedInvocation {
    /// The call.
    #[serde(flatten)]
    pub invocation: Invocation,
    /// The launcher's token, passed on unchanged; absent for every other caller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation: Option<ActivationToken>,
    /// What the router gated the call on, for an action that classifies per call: absent for
    /// every other call. A provider re-derives its classification and refuses with
    /// `ClassificationChanged` when it no longer matches, unless this is the declared ceiling,
    /// which it must accept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classified: Option<crate::classify::CallClass>,
}

/// An XDG activation token: a capability to take focus, handed by the launcher with a `Perform`
/// and by the router to the app. It is never logged or audited, so its `Debug` shows nothing.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActivationToken(String);

impl ActivationToken {
    /// A token, unless it is empty.
    pub fn parse(text: &str) -> Option<Self> {
        (!text.is_empty()).then(|| Self(text.to_owned()))
    }

    /// The token's text, for handing to the compositor.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ActivationToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ActivationToken(..)")
    }
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
    /// Yes, with this token: what an app answers.
    Yes(UndoToken),
    /// Yes, by this journal row (`Run.Undo(id)`): what the router answers its caller once it has
    /// journaled the app's token. An app never answers this.
    Journaled(UndoId),
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
    /// The call is no longer what `Classify` said (the app's state changed in between). The
    /// router gates it again at the declared ceiling.
    ClassificationChanged,
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
