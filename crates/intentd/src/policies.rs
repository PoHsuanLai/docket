//! The policy files on disk: `quire/permissions/` under the data, configuration and system
//! directories. The loader and its rules are `policy-point`'s (pure); this reads the files.
//!
//! - preset `standard`: `<data dir>/quire/permissions/standard/*.cedar`; the first data directory
//!   that has one wins, like the manifests.
//! - person: `$XDG_CONFIG_HOME/quire/permissions/*.cedar`.
//! - assistant: `$XDG_CONFIG_HOME/quire/permissions/assistant/*.cedar` (forbids only).
//! - administrator: `/etc/quire/permissions/*.cedar` (forbids only, never skipped).
//!
//! Files are read in name order. A file that cannot be read is handed to the loader as an
//! unreadable source, so its layer decides what that means.

use policy_point::{Layer, LayerError, Layered, Pdp, PolicySource, SCHEMA};
use std::path::{Path, PathBuf};

/// Where the administrator's policy files are on a real machine.
pub const ADMIN_DIR: &str = "/etc/quire/permissions";

/// The preset in force. There is no setting yet; the next phase adds one.
const PRESET: &str = "standard";

/// The directories the policy files are read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDirs {
    /// The data directories, the person's first; each may hold `quire/permissions/<preset>`.
    pub data_dirs: Vec<PathBuf>,
    /// The person's `quire/permissions` under the configuration directory.
    pub person: PathBuf,
    /// The administrator's directory.
    pub admin: PathBuf,
}

impl PolicyDirs {
    /// The preset directories, in search order.
    fn presets(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.data_dirs
            .iter()
            .map(|d| d.join("quire").join("permissions").join(PRESET))
    }

    /// The assistant's directory, inside the person's.
    pub fn assistant(&self) -> PathBuf {
        self.person.join("assistant")
    }

    /// The directory the preset is read from: the first that holds a policy file.
    pub fn preset(&self) -> Option<PathBuf> {
        self.presets().find(|d| !cedar_files(d).is_empty())
    }
}

/// The `*.cedar` files directly in `dir`, in name order; none when it is not readable.
fn cedar_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "cedar") && p.is_file())
        .collect();
    files.sort();
    files
}

/// The sources of one layer found in `dir`.
fn read_layer(layer: Layer, dir: &Path) -> Vec<PolicySource> {
    cedar_files(dir)
        .into_iter()
        .map(|path| match std::fs::read_to_string(&path) {
            Ok(text) => PolicySource::file(layer, path, text),
            Err(why) => PolicySource::unreadable(layer, path, why.to_string()),
        })
        .collect()
}

/// Every source on disk, with the built-in core. No preset on disk leaves the built-in one.
pub fn read_sources(dirs: &PolicyDirs) -> Vec<PolicySource> {
    let preset = dirs
        .preset()
        .map(|dir| read_layer(Layer::Preset, &dir))
        .unwrap_or_default();
    std::iter::once(PolicySource::core())
        .chain(preset)
        .chain(read_layer(Layer::Person, &dirs.person))
        .chain(read_layer(Layer::Assistant, &dirs.assistant()))
        .chain(read_layer(Layer::Admin, &dirs.admin))
        .collect()
}

/// The policy set the files make now, with what was left out.
pub fn load_policies(dirs: &PolicyDirs) -> Result<Layered, LayerError> {
    Pdp::layered(SCHEMA, &read_sources(dirs))
}
