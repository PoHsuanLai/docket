//! Why a skill is not valid, as typed errors.

use docket_core::ActionRef;

/// A skill that does not parse or break a rule of the format.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkillFault {
    /// A file could not be read.
    #[error("cannot read {file}: {why}")]
    Unreadable {
        /// Which file.
        file: &'static str,
        /// The reader's words.
        why: String,
    },
    /// `skill.toml` is not valid TOML of the expected shape.
    #[error("skill.toml: {0}")]
    Toml(String),
    /// `skill.toml` names a vocabulary this build does not speak.
    #[error("skill.toml: vocab {0} is not 1")]
    Vocab(u32),
    /// An id is not `[a-z0-9-]{{1,48}}`.
    #[error("the id {0:?} is not 1 to 48 lowercase letters, digits and hyphens")]
    BadId(String),
    /// The directory, `skill.toml` and `SKILL.md` do not name the same id.
    #[error("ids disagree: directory {dir:?}, skill.toml {toml:?}, SKILL.md {md:?}")]
    IdMismatch {
        /// The directory name.
        dir: String,
        /// `id` in `skill.toml`.
        toml: String,
        /// `name` in `SKILL.md`.
        md: String,
    },
    /// An entry of `uses` is not `<app>:<action>`.
    #[error("uses entry {0:?} is not <app>:<action>")]
    BadUse(String),
    /// A `when.kinds` entry is not an entity kind.
    #[error("when.kinds entry {0:?} is not an entity kind")]
    BadKind(String),
    /// `SKILL.md` has no front matter.
    #[error("SKILL.md has no `---` front matter")]
    NoFrontMatter,
    /// The front matter has no `name` or no `description`, or it is not on one line.
    #[error("SKILL.md front matter needs a one-line `{0}`")]
    MissingKey(&'static str),
    /// The description is over 160 characters.
    #[error("the description is {0} characters; the limit is 160")]
    DescriptionTooLong(usize),
    /// The body is over 8 KiB.
    #[error("the body is {0} bytes; the limit is 8192")]
    BodyTooLarge(usize),
    /// The body is empty.
    #[error("the body is empty")]
    EmptyBody,
}

/// What a skill's `uses` found among the registered manifests.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Unresolved {
    /// The app, or the action, is not registered: the skill is hidden.
    #[error("{}:{} is not a registered action", .0.app, .0.name)]
    Missing(ActionRef),
}
