//! The two permissions presets, each built into its own policy set.

use crate::layered::LayerError;
use crate::layers::{Layer, PolicySource};
use crate::pdp::{Pdp, PolicyError, SCHEMA};
use docket_core::Permissions;
use std::path::PathBuf;

/// One policy set per preset. Each is the core, that preset's folder and the person's,
/// assistant's and administrator's layers; they are kept apart so a preset stays a folder the
/// person can copy and edit, and a call is ruled by the one the person's setting names.
#[derive(Debug)]
pub struct Presets {
    /// The set for [`Permissions::Auto`].
    pub auto: Pdp,
    /// The set for [`Permissions::Manual`].
    pub manual: Pdp,
}

impl Presets {
    /// The built-in core with each built-in preset: no files are read, so tests and the fake
    /// router get the same grids whatever is installed.
    pub fn built_in() -> Result<Presets, PolicyError> {
        Ok(Presets {
            auto: Pdp::built_in(Permissions::Auto)?,
            manual: Pdp::built_in(Permissions::Manual)?,
        })
    }

    /// The built-in core and presets with the text of the person's and the assistant's rules
    /// files on top: what the files make when they hold exactly this. A source the layers refuse
    /// is left out, like a file on disk would be.
    pub fn with_rules(person: &str, assistant: &str) -> Result<Presets, LayerError> {
        let build = |preset: Permissions| {
            let sources = [
                PolicySource::core(),
                PolicySource::file(Layer::Person, PathBuf::from("never.cedar"), person.to_owned()),
                PolicySource::file(
                    Layer::Assistant,
                    PathBuf::from("assistant.cedar"),
                    assistant.to_owned(),
                ),
            ];
            Pdp::layered_for(SCHEMA, preset, &sources).map(|layered| layered.pdp)
        };
        Ok(Presets {
            auto: build(Permissions::Auto)?,
            manual: build(Permissions::Manual)?,
        })
    }

    /// The set that rules calls under `permissions`.
    pub fn pick(&self, permissions: Permissions) -> &Pdp {
        match permissions {
            Permissions::Auto => &self.auto,
            Permissions::Manual => &self.manual,
        }
    }
}
