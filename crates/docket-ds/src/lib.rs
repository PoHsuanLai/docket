//! The adapter between a quire app's view marks and docket's wire. quire's `ds-intents` is
//! view-only and names no agent type; this crate, in docket's repo, is the one place that
//! converts between the two. Each pair has a total conversion with a test, so a variant added
//! on either side fails the build here and not at a user.
//!
//! - `chips_of`, `keep_of`: the chips a prompt carries and the context the person kept.
//! - `thing_mark`, `entity_ref`: a thing as a row shows it and as the router names it.
//! - [`summon_answer_mark`], [`summon_answer_of`], [`serial_of`], [`serial_mark`].
//! - `DsContextSource`, `DsSummonTarget`: the seams `docket-client` asks ds to answer.
//! - [`summon_origin_mark`]: where a summon came from, as ds's `SummonOriginMark`.
//! - `heard_of`, `DictationBridge`: the voice path from `Voice1.Utterance.Attach` to a field.

mod chips;
mod context;
mod summon;
mod things;
mod voice;

pub use chips::{chips_of, keep_of};
pub use context::{DsContextSource, WindowFacts};
pub use summon::{
    BridgeFault, DsSummonTarget, PromptHost, serial_mark, serial_of, summon_answer_mark,
    summon_answer_of, summon_origin_mark,
};
pub use things::{ThingError, entity_ref, thing_mark};
pub use voice::{DictationBridge, heard_of};
