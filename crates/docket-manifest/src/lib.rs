//! docket's manifest: what an app declares (its entities, relations and actions with their
//! parameters), the validation that turns a [`Manifest`] into a [`ValidManifest`], a manifest
//! file read as TOML (behind the `manifest-file` feature) and a tool call's JSON arguments read
//! into typed ones (`args_from_json`).
//!
//! Pure and portable: serde and `serde_json`, `url` for URL arguments, `docket-vocab` for ids
//! and values, `porter-core` for app names and `prov` for who and what. The TOML parser is only
//! reached with the `manifest-file` feature.

mod args;
mod manifest;
#[cfg(feature = "manifest-file")]
mod manifest_file;
mod validate;

pub use args::{ArgsFault, TARGET_KEY, TargetFault, Why, args_from_json, target_from_json};
pub use manifest::{
    ActionDecl, AgentReach, ArgSink, Cardinality, DryRun, EntityDecl, Filled, IndexPolicy, KeyHint,
    Lasting, Latency, Manifest, ParamDecl, ParamNeed, PropDecl, RELATION_ARG, RelationDecl,
    ResultShape, TargetKind, TitleTrust, UndoSupport, Visibility, related_name,
};
#[cfg(feature = "manifest-file")]
pub use manifest_file::{ManifestFileError, parse};
pub use validate::{ManifestError, ValidManifest, fits, is_offered, open_name, validate};
