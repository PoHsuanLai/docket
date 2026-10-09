//! The ids docket mints or names. Slug ids use `prov`'s grammar; numeric ids are minted by the
//! router and mean nothing outside it.

use porter_core::{AppName, CoreError, is_id};
use prov::{ActionName, EntityId};
use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! text_id {
    ($(#[$doc:meta])* $name:ident, $what:literal, $ok:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// The id written as `text`, or why it is not one.
            pub fn parse(text: &str) -> Result<Self, CoreError> {
                let ok: fn(&str) -> bool = $ok;
                if ok(text) {
                    Ok(Self(text.to_owned()))
                } else {
                    Err(CoreError::MalformedId { what: $what, text: text.to_owned() })
                }
            }

            /// The id's text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = CoreError;
            fn try_from(text: String) -> Result<Self, CoreError> {
                Self::parse(&text)
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> String {
                id.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

macro_rules! number_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub u64);
    };
}

/// `[a-z][a-z0-9_]*`, at most 64 bytes.
fn ident(text: &str) -> bool {
    let mut bytes = text.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && text.len() <= 64
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// 1 to 128 bytes, no control characters: an id docket does not interpret.
fn opaque(text: &str) -> bool {
    !text.is_empty() && text.len() <= 128 && !text.chars().any(char::is_control)
}

/// Words an app wrote for its own UI: 1 to 200 bytes, no control characters.
fn words(text: &str) -> bool {
    !text.is_empty() && text.len() <= 200 && !text.chars().any(char::is_control)
}

text_id!(
    /// An action parameter's name: `[a-z][a-z0-9_]*`.
    ParamName,
    "param name",
    ident
);
text_id!(
    /// The name of a relation an entity kind declares (`from`, `organiser`): `[a-z][a-z0-9_]*`.
    RelationName,
    "relation name",
    ident
);
text_id!(
    /// One option of a choice parameter.
    ChoiceId,
    "choice id",
    is_id
);
text_id!(
    /// App-authored UI words from the manifest (trusted): a label, a plural, a rationale.
    LabelText,
    "label text",
    words
);
text_id!(
    /// The name of an icon in the design system's set.
    IconName,
    "icon name",
    |t| ident(&t.replace(['.', '-'], "_"))
);
text_id!(
    /// An app-minted token for one undoable change, opaque to the router.
    UndoToken,
    "undo token",
    |t| !t.is_empty() && t.len() <= 256 && !t.chars().any(char::is_control)
);
text_id!(
    /// A window, as sill names it for a confirmation anchor or a prompt's parent.
    WindowKey,
    "window key",
    opaque
);
text_id!(
    /// An app-minted reference to a live text field, valid while the field lives.
    TextTargetRef,
    "text target",
    opaque
);
text_id!(
    /// One action offered on an answer card.
    CardActionId,
    "card action id",
    opaque
);
text_id!(
    /// The name of a view in an app (`inbox`, `search`).
    ViewName,
    "view name",
    ident
);
text_id!(
    /// A file, as a document-portal-style token; path text in v1.
    FileRef,
    "file ref",
    |t| !t.is_empty() && t.len() <= 4096 && !t.chars().any(char::is_control)
);
text_id!(
    /// One voice utterance, `u-<n>`, minted by voiced.
    UtteranceId,
    "utterance id",
    is_id
);

impl RelationName {
    /// The choice that names this relation as the `relation` argument of a kind's related
    /// action. A relation name is always a choice id.
    pub fn choice(&self) -> Option<ChoiceId> {
        ChoiceId::parse(self.as_str()).ok()
    }
}

impl ChoiceId {
    /// The id that free text names, if there is exactly one obvious: lowercased, spaces, `-`, `_`
    /// and `.` as `_`, any other character dropped, runs of `_` joined. Text that is already an
    /// id is itself. `None` when nothing is left or the result is too long.
    pub fn slug(text: &str) -> Option<Self> {
        if let Ok(id) = Self::parse(text) {
            return Some(id);
        }
        let mut slug = String::new();
        for c in text.chars() {
            if c.is_ascii_alphanumeric() {
                slug.push(c.to_ascii_lowercase());
            } else if (c.is_whitespace() || matches!(c, '-' | '_' | '.'))
                && !slug.is_empty()
                && !slug.ends_with('_')
            {
                slug.push('_');
            }
        }
        Self::parse(slug.trim_end_matches('_')).ok()
    }
}

number_id!(
    /// One call, minted by the router.
    CallId
);
number_id!(
    /// One user prompt in a session, router-held.
    TurnId
);
number_id!(
    /// One row of the router's undo journal.
    UndoId
);
number_id!(
    /// A router-minted reference to a value the planner may not read.
    Handle
);

/// One step of a plan, as the plan card numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StepId(pub u32);

/// The version of the intents vocabulary: the manifest file format and the wire bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IntentsVocab(pub u32);

impl IntentsVocab {
    /// The vocabulary this build speaks.
    pub const CURRENT: IntentsVocab = IntentsVocab(1);
}

/// One action of one app.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ActionRef {
    /// The app that declares it.
    pub app: AppName,
    /// Its name.
    pub name: ActionName,
}

/// A thing with the words that name it. Titles are third-party text where the kind says so, so
/// they carry a label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityRef {
    /// The thing.
    pub id: EntityId,
    /// Its title.
    pub title: prov::Labelled<String>,
    /// Its subtitle.
    pub subtitle: prov::Labelled<String>,
}

/// The conventional action-name prefix of an app: its last name element, lowercased
/// (`org.quire.Mail` owns `mail.*`).
pub fn action_prefix(app: &AppName) -> String {
    app.as_str()
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_follow_their_grammars() {
        let cases: &[(&str, bool, bool, bool)] = &[
            // text, ParamName, ChoiceId, ViewName
            ("to", true, true, true),
            ("max_count", true, true, true),
            ("Max", false, false, false),
            ("9x", false, true, false),
            ("", false, false, false),
        ];
        for (text, param, choice, view) in cases {
            assert_eq!(ParamName::parse(text).is_ok(), *param, "param {text:?}");
            assert_eq!(ChoiceId::parse(text).is_ok(), *choice, "choice {text:?}");
            assert_eq!(ViewName::parse(text).is_ok(), *view, "view {text:?}");
        }
        assert!(LabelText::parse("Archive").is_ok());
        assert!(LabelText::parse("a\nb").is_err());
        assert!(LabelText::parse(&"x".repeat(201)).is_err());
        assert!(UtteranceId::parse("u-12").is_ok() && UtteranceId::parse("U 12").is_err());
        assert!(IconName::parse("archive-box").is_ok());
    }

    #[test]
    fn free_text_is_slugged_to_the_one_id_it_names() {
        let cases = [
            ("Option A", Some("option_a")),
            ("option_a", Some("option_a")),
            ("  Not -- A!  ", Some("not_a")),
            ("#1 has Option A", Some("1_has_option_a")),
            ("forward", Some("forward")),
            ("???", None),
            ("", None),
            ("é", None),
        ];
        for (text, want) in cases {
            assert_eq!(
                ChoiceId::slug(text).as_ref().map(ChoiceId::as_str),
                want,
                "{text:?}"
            );
        }
        assert!(ChoiceId::slug(&"word ".repeat(40)).is_none(), "too long");
    }

    #[test]
    fn action_prefix_is_the_last_element_lowercased() {
        let cases = [
            ("org.quire.Mail", "mail"),
            ("org.quire.Memory", "memory"),
            ("org.quire.Cua", "cua"),
            ("com.example.WorkLog", "worklog"),
        ];
        for (app, want) in cases {
            let app = AppName::parse(app).expect("app");
            assert_eq!(action_prefix(&app), want);
        }
    }

    #[test]
    fn numeric_ids_serialise_as_plain_numbers() {
        assert_eq!(serde_json::to_string(&CallId(7)).expect("json"), "7");
        assert_eq!(serde_json::to_string(&Handle(3)).expect("json"), "3");
        assert_eq!(
            serde_json::from_str::<UndoId>("41").expect("undo"),
            UndoId(41)
        );
    }
}
