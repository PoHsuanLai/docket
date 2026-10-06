//! Skills: instructions the desktop ships for the companion, with the typed actions they use.
//!
//! A skill teaches the planner and grants nothing: it adds words to the planner's view, never a
//! grant, a Ruling or a widened policy. This crate owns the format (`skill.toml` and `SKILL.md`),
//! discovery from directories the daemon hands in, validation against the registered manifests
//! and the choice of which skills to show. The planner-facing shapes (`SkillCard`, `SkillText`)
//! live in `docket-core`, which stays serde-only.
//!
//! The only way skill text enters is [`discover`] over [`Roots`], and `Roots` holds directory
//! paths from configuration. There is no constructor from a string of skill text outside the two
//! files of an installed directory, so nothing a mail, a page or an agent says can become one.

mod discover;
mod fault;
mod library;
mod skill;

pub use discover::{Found, Rejected, Roots, discover, load_dir};
pub use fault::{SkillFault, Unresolved};
pub use library::{
    BODY_BUDGET_BYTES, Hidden, LOAD_MAX, Library, LoadRefusal, Loaded, PRESELECT_MAX, Reach,
    Situation, preselect, reach, uses_first,
};
pub use skill::{
    Always, BODY_MAX_BYTES, DESCRIPTION_MAX_CHARS, Doc, Facts, Origin, Skill, When, assemble,
    parse, parse_doc, parse_facts,
};
