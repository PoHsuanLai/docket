//! The two permissions presets, each built into its own policy set.

use crate::pdp::{Pdp, PolicyError};
use docket_core::Permissions;

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

    /// The set that rules calls under `permissions`.
    pub fn pick(&self, permissions: Permissions) -> &Pdp {
        match permissions {
            Permissions::Auto => &self.auto,
            Permissions::Manual => &self.manual,
        }
    }
}
