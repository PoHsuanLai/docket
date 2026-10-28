//! A manifest file as text: TOML read into a [`Manifest`] and validated. Behind the
//! `manifest-file` feature, so only the crates that read manifest files carry a TOML parser.

use crate::manifest::Manifest;
use crate::validate::{ManifestError, ValidManifest, validate};

/// Why a manifest was not taken.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestFileError {
    /// The file is not a manifest.
    #[error("manifest file: {0}")]
    Toml(String),
    /// The manifest does not validate.
    #[error("manifest: {0}")]
    Invalid(ManifestError),
}

/// Reads a manifest file and validates it.
pub fn parse(text: &str) -> Result<ValidManifest, ManifestFileError> {
    let manifest: Manifest =
        toml::from_str(text).map_err(|e| ManifestFileError::Toml(e.to_string()))?;
    validate(manifest).map_err(ManifestFileError::Invalid)
}
