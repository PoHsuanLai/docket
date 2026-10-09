//! A second scorer beside the Quick judge, recorded and never obeyed (the first decision-model
//! trial; the plan is "Start with a shadow Quick flagger" in the decision-models report).
//!
//! A [`ShadowFlagger`] reads the same stripped [`ReviewRequest`] the Quick judge reads (the
//! person's turns, the typed action and `{from, size}` of untrusted arguments: no third-party
//! text) and scores how likely the judge should flag it. [`Shadowed`] runs it beside the Quick
//! stage and keeps a [`ShadowNote`]; the verdict it returns is the live reviewer's, untouched.
//!
//! Tighten-only, by type. The only thing a flagger can say is a [`ShadowLean`]: `WouldFlag` or
//! `WouldPass`. Neither converts to a `ReviewVerdict`, so a shadow result cannot become an
//! Allow. The later combined mode is [`either_flags`], which takes the live verdict and a lean
//! and can only turn an `Allow` into an `Ask`; every other verdict, and every failure, comes
//! back as it went in.
//!
//! Promotion rule (from the report; the owner decides, nothing here promotes itself):
//! - Combined ("either one flags") only on a corpus grown well past today's: about 75 harmful
//!   cases per category with no miss bring the Wilson upper bound under 5%.
//! - The shadow replaces the LLM Quick stage only when its false-negative rate is no worse
//!   than the LLM's in every category (injection, overeager, exfiltration, adaptive judge),
//!   with the benign false-positive rate at the chosen threshold acceptable.
//! - Thresholds are tuned per input type; one global cut does not transfer.
//!
//! Second arm: a local encoder (Laya) plugs in as another `ShadowFlagger`; see [`Disabled`].

mod combine;
mod flagger;
mod note;
mod readout;
mod score;
mod watch;

pub use combine::either_flags;
pub use flagger::{Disabled, OptionFlagger, ShadowFlagger};
pub use note::{LiveCall, ShadowLog, ShadowNote, ShadowSink};
pub use readout::{OptionScores, Readout};
pub use score::{DEFAULT_THRESHOLD, ShadowFault, ShadowLean, ShadowScore, Threshold};
pub use watch::Shadowed;
