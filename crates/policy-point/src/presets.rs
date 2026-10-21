//! The permissions preset, built into its policy set.

use crate::layered::LayerError;
use crate::layers::{Layer, PolicySource};
use crate::pdp::{Pdp, PolicyError, SCHEMA};
use std::path::PathBuf;

/// The policy set that rules every call. It is the core, the preset folder and the person's,
/// assistant's and administrator's layers; the preset stays a folder the person can copy and
/// edit. One grid serves both modes: the call's rows (`context.rows`) pick the rules that apply.
#[derive(Debug)]
pub struct Presets {
    /// The set.
    pub pdp: Pdp,
}

impl Presets {
    /// The built-in core and preset: no files are read, so tests and the fake router get the
    /// same grid whatever is installed.
    pub fn built_in() -> Result<Presets, PolicyError> {
        Ok(Presets {
            pdp: Pdp::built_in()?,
        })
    }

    /// The built-in core and preset with the text of the person's and the assistant's rules
    /// files on top: what the files make when they hold exactly this. A source the layers refuse
    /// is left out, like a file on disk would be.
    pub fn with_rules(person: &str, assistant: &str) -> Result<Presets, LayerError> {
        let sources = [
            PolicySource::core(),
            PolicySource::file(
                Layer::Person,
                PathBuf::from("never.cedar"),
                person.to_owned(),
            ),
            PolicySource::file(
                Layer::Assistant,
                PathBuf::from("assistant.cedar"),
                assistant.to_owned(),
            ),
        ];
        Ok(Presets {
            pdp: Pdp::layered(SCHEMA, &sources).map(|layered| layered.pdp)?,
        })
    }

    /// The set that rules calls.
    pub fn pdp(&self) -> &Pdp {
        &self.pdp
    }
}
