//! Confirmation: what the compositor or the shell sheet draws, and what comes back. Every word
//! on it comes from the manifest, the router's own formatting of typed arguments, or the app's
//! dry-run preview; agent prose is never shown, and untrusted values are drawn quoted.

use crate::context::EntityRef;
use crate::ids::{LabelText, WindowKey};
use crate::preview::{FileMove, Preview};
use crate::review::AskReason;
use crate::standing_offer::AlwaysOffer;
use crate::units::Seconds;
use porter_core::{AppName, Count};
use prov::{Actor, ConfirmReceipt, Effect, Source, SpaceId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::future::Future;

pub use prov::ConfirmId;

/// One argument as the sheet shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArgLine {
    /// The parameter's label, from the manifest.
    pub label: LabelText,
    /// Its value.
    pub value: Shown,
}

/// A value as the sheet draws it. `Quoted` is drawn as data, with its source.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Shown {
    /// Typed by the person or authored by the app.
    Plain(String),
    /// Somebody else's words, with where they came from.
    Quoted {
        /// The words.
        text: String,
        /// Whose.
        from: Source,
    },
}

// The words are the person's content: Debug shows the shape only.
impl std::fmt::Debug for Shown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Shown::Plain(t) => write!(f, "Plain(<{} bytes>)", t.len()),
            Shown::Quoted { text, from } => {
                write!(f, "Quoted(<{} bytes> from {from:?})", text.len())
            }
        }
    }
}

/// What the sheet shows of the concrete change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ConfirmDetail {
    /// Who it goes to.
    Recipients(Vec<Shown>),
    /// The text before and after.
    TextDiff {
        /// Before.
        before: Shown,
        /// After.
        after: Shown,
    },
    /// Files that will move.
    Moves(Vec<FileMove>),
    /// Things that will change.
    Entities(Vec<EntityRef>),
    /// Where it goes.
    Destination(Shown),
    /// The app's own preview.
    Preview(Preview),
    /// Nothing beyond the lines.
    Plain,
}

/// Whether the session read anything untrusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum TaintNote {
    /// No.
    Clean,
    /// Yes, from these sources.
    ReadUntrusted(BTreeSet<Source>),
}

/// What the person may choose besides "this once".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmOffer {
    /// Only once (a tainted session, a destructive act).
    OnceOnly,
    /// Once, or always for this app, class and Space.
    OnceOrAlways,
    /// Once, or "allow from the terminal: this action, until logout" (a call made through
    /// `quire-do`; never offered for a destructive act or an action that asks every time).
    OnceOrFromTerminal,
}

/// How the person must answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gesture {
    /// Press the button.
    Press,
    /// Hold it (destructive acts).
    HoldToConfirm,
}

/// Where the sheet appears.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Anchor {
    /// On this window.
    Window(WindowKey),
    /// On the launcher.
    Launcher,
    /// Centred.
    Centre,
}

/// What the compositor or the shell draws.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmRequest {
    /// Which confirmation.
    pub id: ConfirmId,
    /// The Space.
    pub space: SpaceId,
    /// Who is acting.
    pub actor: Actor,
    /// The app acted on.
    pub app: AppName,
    /// "Forward 12 messages", the manifest's label.
    pub action: LabelText,
    /// Its effect.
    pub effect: Effect,
    /// How many things.
    pub count: Count,
    /// The concrete change.
    pub detail: ConfirmDetail,
    /// The arguments.
    pub lines: Vec<ArgLine>,
    /// Why it asks, as short fixed phrases.
    pub why: Vec<AskReason>,
    /// Whether the session read untrusted content.
    pub taint: TaintNote,
    /// What the person may choose.
    pub offer: ConfirmOffer,
    /// Whether "allow always" is offered as a scoped standing grant, and for what scope; for the
    /// callers that hold one (an editor, an ACP agent) it replaces the broad `OnceOrAlways`. A
    /// sheet from before it existed reads as none.
    #[serde(default)]
    pub always: AlwaysOffer,
    /// How to answer.
    pub gesture: Gesture,
    /// Where to draw it.
    pub anchor: Anchor,
    /// How long it stays (the setting `agent.confirm.expiry_s`, proposed 120).
    pub expires: Seconds,
}

/// How a grant lasts.
pub use porter_core::consent::GrantScope;

/// The answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ConfirmAnswer {
    /// Yes, with the receipt that proves the person said so.
    Allowed {
        /// Once or always.
        scope: GrantScope,
        /// The proof.
        receipt: ConfirmReceipt,
    },
    /// Yes, and from now until logout the same action may run from the terminal without
    /// asking. Only a call made through `quire-do` honours it; the router drops it for any other
    /// caller.
    AllowedFromTerminal {
        /// The proof.
        receipt: ConfirmReceipt,
    },
    /// No answer to act on.
    Ended(ConfirmEnd),
}

impl ConfirmAnswer {
    /// For a sheet that did not offer "allow from the terminal" (a computer-use step, a widened
    /// policy): that answer was never on it, so it counts as the router withdrawing the sheet.
    pub fn without_terminal_grant(self) -> ConfirmAnswer {
        match self {
            ConfirmAnswer::AllowedFromTerminal { .. } => {
                ConfirmAnswer::Ended(ConfirmEnd::Cancelled)
            }
            other => other,
        }
    }
}

/// How a confirmation ended without a yes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmEnd {
    /// The person said no.
    Refused,
    /// Closed without an answer.
    Dismissed,
    /// Timed out.
    Expired,
    /// The router withdrew it (a halt, a cancel).
    Cancelled,
}

/// The seam that draws confirmations. Drawing them is not docket's (sill phase A, the trusted
/// surface phase B).
pub trait Confirmer: Send + Sync {
    /// Asks the person.
    fn confirm(&self, request: ConfirmRequest) -> impl Future<Output = ConfirmAnswer> + Send;
    /// Withdraws a pending request.
    fn cancel(&self, id: &ConfirmId) -> impl Future<Output = ()> + Send;
}
