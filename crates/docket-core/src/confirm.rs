//! Confirmation: what the compositor or the shell sheet draws, and what comes back. Every word
//! on it comes from the manifest, the router's own formatting of typed arguments, or the app's
//! dry-run preview; agent prose is never shown, and untrusted values are drawn quoted.

use crate::brief::{BriefSheet, BriefText};
use crate::helper::HelperName;
use crate::limit::LimitNote;
use crate::preview::{FileMove, Preview};
use crate::review::AskReason;
use crate::standing_offer::AlwaysOffer;
use docket_vocab::{EntityRef, LabelText, Seconds, WindowKey};
use porter_core::{AppName, Count};
use prov::{Actor, ClientName, ConfirmReceipt, Effect, SessionId, Source, SpaceId};
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

/// Where a sheet for a call in an editor's session goes: the app behind the editor's connection,
/// as the router saw it (never a name the editor wrote about itself), and the session, so the
/// editor's process shows the sheet in the right one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorRoute {
    /// The editor's app.
    pub client: ClientName,
    /// The session whose call asks.
    pub session: SessionId,
}

/// Whether the sheet offers "never" beside its other answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NeverOffer {
    /// Not on the sheet (the person's own acts, a sheet that is not about an agent's call).
    #[default]
    Withheld,
    /// On the sheet: the answer refuses this call and keeps a "never" for it.
    Offered,
}

/// What the sheet says about limits: whether "never" is offered, and the limit that is the
/// reason it asks, if one is.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SheetLimits {
    /// Whether "never" is offered.
    pub never: NeverOffer,
    /// The "always ask" entry that made this sheet appear, in plain words, with the id Undo
    /// removes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<LimitNote>,
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
    /// Whether "never" is offered, and the limit behind the sheet, if one is. A sheet from
    /// before limits reads as offering nothing.
    #[serde(default)]
    pub limits: SheetLimits,
    /// How to answer.
    pub gesture: Gesture,
    /// Where to draw it.
    pub anchor: Anchor,
    /// How long it stays (the setting `agent.confirm.expiry_s`, proposed 120).
    pub expires: Seconds,
    /// The editor the person is speaking through, when the sheet is for a call in a session whose
    /// latest turn came from an editor. The
    /// confirmer hands such a sheet to that app's process rather than to the desktop. Sheets
    /// from before it existed read as none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<EditorRoute>,
    /// The helper the call is a job of, by the name the person gave it: the sheet says it is the
    /// one asking. Sheets from before helpers read as none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub helper: Option<HelperName>,
    /// For a sheet that asks the person to check a brief: the draft as an editable field, with
    /// the stretches to look at. A surface that does not know it draws a plain sheet from
    /// `lines`, and its answer carries no words, so it confirms nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief: Option<BriefSheet>,
}

/// What every sheet must say. A consumer outside docket builds a request from this with
/// [`ConfirmRequest::new`] rather than a struct literal, so a field added with a default (like
/// `always`) never breaks it; a new required field lands here, and breaking then is intended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmParts {
    /// Which confirmation.
    pub id: ConfirmId,
    /// The Space.
    pub space: SpaceId,
    /// Who is acting.
    pub actor: Actor,
    /// The app acted on.
    pub app: AppName,
    /// The manifest's label.
    pub action: LabelText,
    /// Its effect.
    pub effect: Effect,
    /// How many things.
    pub count: Count,
    /// The concrete change.
    pub detail: ConfirmDetail,
    /// The arguments.
    pub lines: Vec<ArgLine>,
    /// Why it asks.
    pub why: Vec<AskReason>,
    /// Whether the session read untrusted content.
    pub taint: TaintNote,
    /// What the person may choose.
    pub offer: ConfirmOffer,
    /// How to answer.
    pub gesture: Gesture,
    /// Where to draw it.
    pub anchor: Anchor,
    /// How long it stays.
    pub expires: Seconds,
}

impl ConfirmRequest {
    /// A request with no standing-grant offer; add one with [`ConfirmRequest::offering`].
    pub fn new(parts: ConfirmParts) -> Self {
        let ConfirmParts {
            id,
            space,
            actor,
            app,
            action,
            effect,
            count,
            detail,
            lines,
            why,
            taint,
            offer,
            gesture,
            anchor,
            expires,
        } = parts;
        ConfirmRequest {
            id,
            space,
            actor,
            app,
            action,
            effect,
            count,
            detail,
            lines,
            why,
            taint,
            offer,
            always: AlwaysOffer::default(),
            limits: SheetLimits::default(),
            gesture,
            anchor,
            expires,
            editor: None,
            helper: None,
            brief: None,
        }
    }

    /// The same request for a brief the person is to check.
    pub fn for_brief(self, brief: Option<BriefSheet>) -> Self {
        ConfirmRequest { brief, ..self }
    }

    /// The same request for a call in a session an editor drives.
    pub fn for_editor(self, editor: Option<EditorRoute>) -> Self {
        ConfirmRequest { editor, ..self }
    }

    /// The same request for a call one of the person's helpers makes.
    pub fn for_helper(self, helper: Option<HelperName>) -> Self {
        ConfirmRequest { helper, ..self }
    }

    /// The same request with these limits.
    pub fn limiting(self, limits: SheetLimits) -> Self {
        ConfirmRequest { limits, ..self }
    }

    /// The same request with this "allow always" offer.
    pub fn offering(self, always: AlwaysOffer) -> Self {
        ConfirmRequest { always, ..self }
    }
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
        /// For a brief sheet: the field's content when the person pressed the button. Every
        /// other sheet answers none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        written: Option<BriefText>,
    },
    /// Yes, and from now until logout the same action may run from the terminal without
    /// asking. Only a call made through `quire-do` honours it; the router drops it for any other
    /// caller.
    AllowedFromTerminal {
        /// The proof.
        receipt: ConfirmReceipt,
    },
    /// Never: refuse this call and keep a "never" for it. The receipt proves the person said so,
    /// as it does for a yes. It is a new answer rather than a scope because a scope says how long
    /// a yes lasts (`GrantScope` is porter's and has no no), and a no that is kept is not a
    /// `ConfirmEnd`, which says no answer was given.
    Never {
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

    /// For a sheet that did not offer "never": that answer was never on it, so it counts as the
    /// person saying no this once, and nothing is kept.
    pub fn without_never(self) -> ConfirmAnswer {
        match self {
            ConfirmAnswer::Never { .. } => ConfirmAnswer::Ended(ConfirmEnd::Refused),
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
