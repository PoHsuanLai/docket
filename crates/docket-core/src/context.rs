//! What the person is looking at, as an app reports it, and the view a planner may see.
//!
//! [`ContextSnapshot`] is the app's answer (text carries labels). [`ContextView`] is what the
//! router derives for a companion session: every untrusted text is a [`Reveal::Handle`]. Only
//! the router builds a view, so a model never reads text it was not meant to.

use crate::ids::{Handle, TextTargetRef, ViewName};
use crate::units::CharCount;
use porter_core::{AppName, Count};
use prov::{EntityId, EntityKind, Labelled};
use serde::{Deserialize, Serialize};

pub use crate::ids::EntityRef;

/// A value shown to a reader, or a handle to it when the reader may not read it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Reveal<T> {
    /// The value itself.
    Plain(T),
    /// A reference the router resolves for the reader or for the screen, never for a planner.
    Handle(Handle),
}

impl<T> Reveal<T> {
    /// Transforms a plain value; a handle stays a handle.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Reveal<U> {
        match self {
            Reveal::Plain(v) => Reveal::Plain(f(v)),
            Reveal::Handle(h) => Reveal::Handle(h),
        }
    }

    /// The handle, if this is one.
    pub fn handle(&self) -> Option<Handle> {
        match self {
            Reveal::Plain(_) => None,
            Reveal::Handle(h) => Some(*h),
        }
    }
}

/// What the user is looking at in one window: the `Context` call's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSnapshot {
    /// The app.
    pub app: AppName,
    /// The window title.
    pub window: Labelled<String>,
    /// Where in the app.
    pub here: Here,
    /// What is selected.
    pub selection: Selection,
    /// What is on screen.
    pub visible: Visible,
    /// The editable text field, if any.
    pub text_target: TextTarget,
    /// `Private` windows are reported as the app alone.
    pub privacy: WindowPrivacy,
}

/// Where in an app the person is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Here {
    /// Nowhere in particular.
    Nowhere,
    /// Looking at one thing.
    Entity(EntityRef),
    /// In a view, perhaps with a query.
    View {
        /// Which view.
        view: ViewName,
        /// What they searched for.
        query: Option<Labelled<String>>,
    },
}

/// What is selected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Selection {
    /// Nothing.
    Nothing,
    /// These things.
    Entities {
        /// Their kind.
        kind: EntityKind,
        /// Which.
        items: Vec<EntityRef>,
    },
    /// Some text.
    Text(Labelled<String>),
    /// These files.
    Files(Vec<crate::ids::FileRef>),
}

/// What is on screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Visible {
    /// The kind of the listed things, if one.
    pub kind: Option<EntityKind>,
    /// Those in view (capped by the app).
    pub items: Vec<EntityRef>,
    /// How many there are in all.
    pub total: Count,
}

/// An editable field, if the focus is in one. Password and PIN fields are never reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum TextTarget {
    /// No field.
    None,
    /// This field.
    Field(EditTarget),
}

/// A live text field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditTarget {
    /// The app's token for the field.
    pub field: TextTargetRef,
    /// What the field is for.
    pub purpose: TextPurpose,
    /// The selected span.
    pub selection: CharRange,
    /// The text around the caret.
    pub around: Labelled<String>,
}

/// A span of characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CharRange {
    /// The first character.
    pub from: CharCount,
    /// One past the last.
    pub to: CharCount,
}

/// What a text field is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextPurpose {
    /// Plain text.
    Plain,
    /// Rich text.
    Rich,
    /// Code.
    Code,
    /// A search box.
    Search,
}

/// Whether a window may be described to the companion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowPrivacy {
    /// Ordinary.
    Normal,
    /// Private: the router drops everything but the app.
    Private,
}

/// What a context call covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextScope {
    /// The window the person is in.
    ActiveWindow,
}

/// Whether the person kept a chip of context with their prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Keep {
    /// Sent with the prompt.
    Kept,
    /// Removed by the person.
    Dropped,
}

/// Which chips of context the person kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContextKeep {
    /// The search query.
    pub query: Keep,
    /// The visible results.
    pub results: Keep,
    /// The selection.
    pub selection: Keep,
    /// The window.
    pub window: Keep,
}

/// A thing with its words as a reader may see them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityLine {
    /// The thing.
    pub id: EntityId,
    /// Its title.
    pub title: Reveal<String>,
    /// Its subtitle.
    pub subtitle: Reveal<String>,
}

/// [`Here`] as a reader sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum HereView {
    /// Nowhere in particular.
    Nowhere,
    /// Looking at one thing.
    Entity(EntityLine),
    /// In a view.
    View {
        /// Which view.
        view: ViewName,
        /// What they searched for.
        query: Option<Reveal<String>>,
    },
}

/// [`Selection`] as a reader sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum SelectionView {
    /// Nothing.
    Nothing,
    /// These things.
    Entities {
        /// Their kind.
        kind: EntityKind,
        /// Which.
        items: Vec<EntityLine>,
    },
    /// Some text.
    Text(Reveal<String>),
    /// These files.
    Files(Vec<crate::ids::FileRef>),
}

/// [`Visible`] as a reader sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisibleView {
    /// The kind of the listed things, if one.
    pub kind: Option<EntityKind>,
    /// Those in view.
    pub items: Vec<EntityLine>,
    /// How many there are in all.
    pub total: Count,
}

/// [`TextTarget`] as a reader sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum TextTargetView {
    /// No field.
    None,
    /// This field.
    Field {
        /// The app's token for the field.
        field: TextTargetRef,
        /// What it is for.
        purpose: TextPurpose,
        /// The selected span.
        selection: CharRange,
        /// The text around the caret.
        around: Reveal<String>,
    },
}

/// A [`ContextSnapshot`] with untrusted text replaced by handles: all a planner sees of where
/// the person is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextView {
    /// The app.
    pub app: AppName,
    /// The window title.
    pub window: Reveal<String>,
    /// Where in the app.
    pub here: HereView,
    /// What is selected.
    pub selection: SelectionView,
    /// What is on screen.
    pub visible: VisibleView,
    /// The editable field, if any.
    pub text_target: TextTargetView,
}
