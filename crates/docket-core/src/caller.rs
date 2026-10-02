//! Who is calling the router, and which member of `Intents1` a request is. Roles come from
//! `intentd.toml` and the connection's identity; they are never sent in a body.

use porter_core::AppId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The role a connection plays, configured per bus name in `intentd.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallerRole {
    /// sill's launcher: acts as the person.
    Launcher,
    /// A quire app recording a turn from its own prompt field.
    Field,
    /// companiond.
    Companion,
    /// readerd: the only caller that may resolve a handle.
    Reader,
    /// cuad.
    Cua,
    /// actions-mcp.
    Mcp,
    /// sill as the only `Confirm1` server the router trusts.
    Confirm,
    /// The compositor fork: may halt.
    Compositor,
    /// sill's control centre: may halt and resume.
    Control,
    /// Any other app: its own actions, plus search, preview and suggest.
    App,
}

impl CallerRole {
    /// Every role.
    pub const ALL: [CallerRole; 10] = [
        CallerRole::Launcher,
        CallerRole::Field,
        CallerRole::Companion,
        CallerRole::Reader,
        CallerRole::Cua,
        CallerRole::Mcp,
        CallerRole::Confirm,
        CallerRole::Compositor,
        CallerRole::Control,
        CallerRole::App,
    ];
}

/// A caller as the transport derived it: an identity and the roles `intentd.toml` gives that
/// bus name. One name may play several (sill is the launcher, the confirm server and the
/// control centre); the role a call acts in is the first that may make it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallerId {
    /// The application, as porter derives it from the connection.
    pub app: AppId,
    /// The roles of the name; empty means a plain app.
    pub roles: BTreeSet<CallerRole>,
}

/// One member of `org.quire.Intents1`, as a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Member {
    /// `.Registry.Manifests`.
    Manifests,
    /// `.Index.Push`.
    IndexPush,
    /// `.Index.Reset`.
    IndexReset,
    /// `.Search.Query`.
    SearchQuery,
    /// `.Search.Cancel`.
    SearchCancel,
    /// `.Run.Perform`.
    Perform,
    /// `.Run.Preview`.
    Preview,
    /// `.Run.Suggest`.
    Suggest,
    /// `.Run.Undo`.
    Undo,
    /// `.Run.UndoAll`.
    UndoAll,
    /// `.Context.Current`.
    Context,
    /// `.Session.Open`.
    SessionOpen,
    /// `.Session.Turn`.
    SessionTurn,
    /// `.Session.Close`.
    SessionClose,
    /// `.Session.Resolve`.
    SessionResolve,
    /// `.Session.Display`.
    SessionDisplay,
    /// `.Session.Read`.
    SessionRead,
    /// `.Session.TaskPolicy`.
    SessionTaskPolicy,
    /// `.Session.Widen`.
    SessionWiden,
    /// `.Session.Note`.
    SessionNote,
    /// `.Session.Recall`.
    SessionRecall,
    /// `.Message.Send`.
    MessageSend,
    /// `.Message.Inbox`.
    MessageInbox,
    /// `.Gate.Grant`.
    GateGrant,
    /// `.Gate.Check`.
    GateCheck,
    /// `.Control.Halt`.
    ControlHalt,
    /// `.Control.Resume`.
    ControlResume,
    /// `.Control.State`.
    ControlState,
    /// `.Control.Journal`.
    ControlJournal,
}

impl Member {
    /// Every member, in the order the interface table lists them.
    pub const ALL: [Member; 29] = [
        Member::Manifests,
        Member::IndexPush,
        Member::IndexReset,
        Member::SearchQuery,
        Member::SearchCancel,
        Member::Perform,
        Member::Preview,
        Member::Suggest,
        Member::Undo,
        Member::UndoAll,
        Member::Context,
        Member::SessionOpen,
        Member::SessionTurn,
        Member::SessionClose,
        Member::SessionResolve,
        Member::SessionDisplay,
        Member::SessionRead,
        Member::SessionTaskPolicy,
        Member::SessionWiden,
        Member::SessionNote,
        Member::SessionRecall,
        Member::MessageSend,
        Member::MessageInbox,
        Member::GateGrant,
        Member::GateCheck,
        Member::ControlHalt,
        Member::ControlResume,
        Member::ControlState,
        Member::ControlJournal,
    ];
}

impl crate::wire::IntentsRequest {
    /// The member this request calls.
    pub fn member(&self) -> Member {
        use crate::wire::IntentsRequest as R;
        match self {
            R::Manifests => Member::Manifests,
            R::IndexPush(_) => Member::IndexPush,
            R::IndexReset { .. } => Member::IndexReset,
            R::Search(_) => Member::SearchQuery,
            R::SearchCancel(_) => Member::SearchCancel,
            R::Perform { .. } => Member::Perform,
            R::Preview(_) => Member::Preview,
            R::Suggest(_) => Member::Suggest,
            R::Undo(_) => Member::Undo,
            R::UndoAll(_) => Member::UndoAll,
            R::Context { .. } => Member::Context,
            R::SessionOpen(_) => Member::SessionOpen,
            R::SessionTurn { .. } => Member::SessionTurn,
            R::SessionClose { .. } => Member::SessionClose,
            R::SessionResolve { .. } => Member::SessionResolve,
            R::SessionDisplay { .. } => Member::SessionDisplay,
            R::SessionRead { .. } => Member::SessionRead,
            R::SessionTaskPolicy { .. } => Member::SessionTaskPolicy,
            R::SessionWiden { .. } => Member::SessionWiden,
            R::SessionNote { .. } => Member::SessionNote,
            R::SessionRecall { .. } => Member::SessionRecall,
            R::MessageSend { .. } => Member::MessageSend,
            R::MessageInbox(_) => Member::MessageInbox,
            R::GateGrant(_) => Member::GateGrant,
            R::GateCheck(_) => Member::GateCheck,
            R::ControlHalt { .. } => Member::ControlHalt,
            R::ControlResume { .. } => Member::ControlResume,
            R::ControlState => Member::ControlState,
            R::ControlJournal(_) => Member::ControlJournal,
        }
    }
}
