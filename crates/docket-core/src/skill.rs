//! What a skill is to the planner: a catalogue line and, once expanded or loaded, a body of
//! trusted text. The files, their parsing and their validation are `docket-skills`; the planner
//! contract only needs these shapes. A skill adds words to the view and grants nothing.

use prov::{Confidentiality, Integrity, Label, Source};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The longest skill id.
pub const SKILL_ID_MAX: usize = 48;

/// Why text is not a skill id.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("a skill id is 1 to 48 lowercase letters, digits and hyphens")]
pub struct SkillIdError;

/// A skill's id: `[a-z0-9-]{1,48}`, also its directory name and the `name` of its `SKILL.md`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SkillId(String);

impl SkillId {
    /// `text` as an id.
    pub fn parse(text: &str) -> Result<Self, SkillIdError> {
        let fits = !text.is_empty() && text.len() <= SKILL_ID_MAX;
        let chars = text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if fits && chars {
            Ok(Self(text.to_owned()))
        } else {
            Err(SkillIdError)
        }
    }

    /// The id.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for SkillId {
    type Error = SkillIdError;
    fn try_from(text: String) -> Result<Self, SkillIdError> {
        Self::parse(&text)
    }
}

impl From<SkillId> for String {
    fn from(id: SkillId) -> String {
        id.0
    }
}

impl std::fmt::Display for SkillId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The version a skill's owner gave it (`0.1.0`), as written.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SkillVersion(pub String);

/// One line of the catalogue: what the planner may choose to load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillCard {
    /// Its id, the argument of `companion.skill.load`.
    pub id: SkillId,
    /// The catalogue line (at most 160 characters).
    pub description: String,
}

/// A skill's body as the planner reads it: trusted text with its source. Shipped text is the
/// owning app's; a person's own skill is the person's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillText {
    /// Which skill.
    pub id: SkillId,
    /// Always integrity Trusted; the source is the owner app or the person.
    pub label: Label,
    /// The Markdown body.
    pub body: String,
}

impl SkillText {
    /// A body from `source`, labelled Trusted. Neither source is ever an instruction to the
    /// reader: this is text for the planner's view only.
    pub fn new(id: SkillId, source: Source, body: String) -> Self {
        Self {
            id,
            label: Label {
                integrity: Integrity::Trusted,
                confidentiality: Confidentiality::Public,
                classes: BTreeSet::new(),
                sources: BTreeSet::from([source]),
            },
            body,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_follow_their_grammar() {
        let long = "a".repeat(49);
        let cases = [
            ("windows-spaces", true),
            ("a", true),
            ("x9", true),
            ("", false),
            ("Windows", false),
            ("a_b", false),
            ("a b", false),
            ("../x", false),
            (long.as_str(), false),
        ];
        for (text, ok) in cases {
            assert_eq!(SkillId::parse(text).is_ok(), ok, "{text:?}");
        }
    }

    #[test]
    fn a_skill_text_is_trusted_with_its_source() {
        let id = SkillId::parse("x").expect("id");
        let text = SkillText::new(id, Source::User, "body".to_owned());
        assert_eq!(text.label.integrity, Integrity::Trusted);
        assert_eq!(text.label.sources, BTreeSet::from([Source::User]));
    }
}
