//! The layers a policy set is made of, and the sources that fill them.
//!
//! The order is the order of loading: the core is ours and compiled in, a preset is the grid, the
//! person adds to it, the assistant and the administrator may only add rules that ask or refuse.
//! Reading files is the caller's: this crate is pure and takes the text.

use crate::pdp::{CORE_POLICIES, preset_text};
use docket_core::Permissions;
use std::fmt;
use std::path::PathBuf;

/// One layer of the policy set, in loading order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// The safety invariants that hold in every preset (compiled in).
    Core,
    /// The grid: which calls are final, judged or asked.
    Preset,
    /// The person's own rules; anything the schema allows.
    Person,
    /// Rules the assistant wrote; forbids only.
    Assistant,
    /// The administrator's rules; forbids only, and never skipped.
    Admin,
}

impl Layer {
    /// Whether a rule of this layer may only forbid: a permit there is refused.
    pub fn forbids_only(self) -> bool {
        matches!(self, Layer::Assistant | Layer::Admin)
    }
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Layer::Core => "core",
            Layer::Preset => "preset",
            Layer::Person => "person",
            Layer::Assistant => "assistant",
            Layer::Admin => "administrator",
        })
    }
}

/// Where a source came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyOrigin {
    /// Compiled into this crate.
    BuiltIn(&'static str),
    /// A file.
    File(PathBuf),
}

impl fmt::Display for PolicyOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PolicyOrigin::BuiltIn(name) => write!(f, "built-in {name}"),
            PolicyOrigin::File(path) => write!(f, "{}", path.display()),
        }
    }
}

/// The text of one policy file and where it belongs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicySource {
    /// The layer it fills.
    pub layer: Layer,
    /// Where the text came from, for messages.
    pub origin: PolicyOrigin,
    /// The Cedar text, or why the file could not be read. An unreadable source is refused by its
    /// layer like a bad one, so a file that cannot be read never loosens the set.
    pub text: Result<String, String>,
}

impl PolicySource {
    /// The compiled-in core.
    pub fn core() -> Self {
        Self::built_in(Layer::Core, "core", CORE_POLICIES)
    }

    /// The compiled-in preset: what stands in for a preset that is missing or bad.
    pub fn preset(preset: Permissions) -> Self {
        let name = match preset {
            Permissions::Auto => "Auto preset",
            Permissions::Manual => "Manual preset",
        };
        Self::built_in(Layer::Preset, name, preset_text(preset))
    }

    /// A file's text.
    pub fn file(layer: Layer, path: PathBuf, text: String) -> Self {
        Self {
            layer,
            origin: PolicyOrigin::File(path),
            text: Ok(text),
        }
    }

    /// A file that could not be read, and why.
    pub fn unreadable(layer: Layer, path: PathBuf, why: String) -> Self {
        Self {
            layer,
            origin: PolicyOrigin::File(path),
            text: Err(why),
        }
    }

    fn built_in(layer: Layer, name: &'static str, text: &str) -> Self {
        Self {
            layer,
            origin: PolicyOrigin::BuiltIn(name),
            text: Ok(text.to_owned()),
        }
    }
}

/// A source that was refused, and why. The reason names the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    /// The layer of the source.
    pub layer: Layer,
    /// The source.
    pub origin: PolicyOrigin,
    /// Why, in a plain sentence.
    pub why: String,
}

impl fmt::Display for Rejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({} layer): {}", self.origin, self.layer, self.why)
    }
}
