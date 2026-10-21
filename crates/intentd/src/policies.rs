//! The policy files on disk: `quire/permissions/` under the data, configuration and system
//! directories. The loader and its rules are `policy-point`'s (pure); this reads the files.
//!
//! - the preset `default`, the one grid both modes use: `<data dir>/quire/permissions/default/*.cedar`;
//!   the first data directory that has one wins, like the manifests, so a copy under
//!   `$XDG_DATA_HOME` is the person's own edit of it. An edited `auto` or `manual` folder from
//!   before the two modes shared a grid is not read; it is named as unused.
//! - person: `$XDG_CONFIG_HOME/quire/permissions/*.cedar`.
//! - assistant: `$XDG_CONFIG_HOME/quire/permissions/assistant/*.cedar` (forbids only).
//! - administrator: `/etc/quire/permissions/*.cedar` (forbids only, never skipped).
//!
//! Files are read in name order. A file that cannot be read is handed to the loader as an
//! unreadable source, so its layer decides what that means. One policy set is built, from the
//! core, the preset, and the person's, assistant's and administrator's layers.

use policy_point::{Layer, LayerError, Pdp, PolicySource, Presets, Rejected, SCHEMA};
use std::fmt;
use std::path::{Path, PathBuf};

/// The folder of the preset under `quire/permissions/`.
const PRESET_FOLDER: &str = "default";

/// The folders of the two presets from before the modes shared one grid.
const OLD_FOLDERS: [&str; 2] = ["auto", "manual"];

/// Where the administrator's policy files are on a real machine.
pub const ADMIN_DIR: &str = "/etc/quire/permissions";

/// Where the rules of a preset in force came from, in words for the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetOrigin {
    /// The copy built into the program: no folder is installed, or the one there did not load.
    BuiltIn,
    /// The folder installed for the whole system.
    System,
    /// A folder under the person's own data directory: theirs to edit.
    EditedByYou,
}

impl fmt::Display for PresetOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PresetOrigin::BuiltIn => "built in",
            PresetOrigin::System => "from the system",
            PresetOrigin::EditedByYou => "edited by you",
        })
    }
}

/// The directories the policy files are read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDirs {
    /// The person's data directory: a preset copied here is the person's own edit.
    pub data_home: PathBuf,
    /// The system data directories, in search order after the person's.
    pub system_data: Vec<PathBuf>,
    /// The person's `quire/permissions` under the configuration directory.
    pub person: PathBuf,
    /// The administrator's directory.
    pub admin: PathBuf,
}

impl PolicyDirs {
    /// The directories that may hold the preset, in search order, each with whose it is.
    fn presets(&self) -> impl Iterator<Item = (PathBuf, PresetOrigin)> + '_ {
        let mine = std::iter::once((&self.data_home, PresetOrigin::EditedByYou));
        let system = self.system_data.iter().map(|d| (d, PresetOrigin::System));
        mine.chain(system).map(|(d, origin)| {
            (
                d.join("quire").join("permissions").join(PRESET_FOLDER),
                origin,
            )
        })
    }

    /// The assistant's directory, inside the person's.
    pub fn assistant(&self) -> PathBuf {
        self.person.join("assistant")
    }

    /// The directory the preset is read from, and whose it is: the first that holds a policy file.
    pub fn preset(&self) -> Option<(PathBuf, PresetOrigin)> {
        self.presets().find(|(d, _)| !cedar_files(d).is_empty())
    }

    /// The preset directory that exists, for the watch.
    pub fn preset_dirs(&self) -> Vec<PathBuf> {
        self.preset().map(|(dir, _)| dir).into_iter().collect()
    }

    /// The `auto` and `manual` folders the person edited under their data directory before the
    /// modes shared one grid. They are no longer read.
    pub fn unused_presets(&self) -> Vec<PathBuf> {
        OLD_FOLDERS
            .iter()
            .map(|name| self.data_home.join("quire").join("permissions").join(name))
            .filter(|dir| !cedar_files(dir).is_empty())
            .collect()
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
    let folder = dirs
        .preset()
        .map(|(dir, _)| read_layer(Layer::Preset, &dir))
        .unwrap_or_default();
    std::iter::once(PolicySource::core())
        .chain(folder)
        .chain(read_layer(Layer::Person, &dirs.person))
        .chain(read_layer(Layer::Assistant, &dirs.assistant()))
        .chain(read_layer(Layer::Admin, &dirs.admin))
        .collect()
}

/// The policy set the files make now, with what was left out and where the preset came from.
#[derive(Debug)]
pub struct LoadedPolicies {
    /// The set.
    pub presets: Presets,
    /// The sources left out, each once.
    pub skipped: Vec<Rejected>,
    /// Where the preset in force came from.
    pub preset: PresetOrigin,
    /// Edited `auto` or `manual` folders that are no longer read.
    pub unused: Vec<PathBuf>,
}

/// The origin the preset has once the loader has had its say: a folder that was skipped is the
/// built-in copy.
fn origin(dirs: &PolicyDirs, skipped: &[Rejected]) -> PresetOrigin {
    let dropped = skipped.iter().any(|r| r.layer == Layer::Preset);
    match dirs.preset() {
        Some((_, origin)) if !dropped => origin,
        Some(_) | None => PresetOrigin::BuiltIn,
    }
}

/// The policy set the files make now.
pub fn load_policies(dirs: &PolicyDirs) -> Result<LoadedPolicies, LayerError> {
    let layered = Pdp::layered(SCHEMA, &read_sources(dirs))?;
    Ok(LoadedPolicies {
        preset: origin(dirs, &layered.skipped),
        presets: Presets { pdp: layered.pdp },
        skipped: layered.skipped,
        unused: dirs.unused_presets(),
    })
}
