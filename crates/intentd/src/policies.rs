//! The policy files on disk: `quire/permissions/` under the data, configuration and system
//! directories. The loader and its rules are `policy-point`'s (pure); this reads the files.
//!
//! - presets `auto` and `manual`: `<data dir>/quire/permissions/<preset>/*.cedar`; the first data
//!   directory that has one wins, like the manifests, so a copy under `$XDG_DATA_HOME` is the
//!   person's own edit of it.
//! - person: `$XDG_CONFIG_HOME/quire/permissions/*.cedar`.
//! - assistant: `$XDG_CONFIG_HOME/quire/permissions/assistant/*.cedar` (forbids only).
//! - administrator: `/etc/quire/permissions/*.cedar` (forbids only, never skipped).
//!
//! Files are read in name order. A file that cannot be read is handed to the loader as an
//! unreadable source, so its layer decides what that means. One policy set is built per preset,
//! each from the same core, person, assistant and administrator layers.

use docket_core::Permissions;
use policy_point::{Layer, LayerError, Pdp, PolicySource, Presets, Rejected, SCHEMA};
use std::fmt;
use std::path::{Path, PathBuf};

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
    /// The directories that may hold `preset`, in search order, each with whose it is.
    fn presets(&self, preset: Permissions) -> impl Iterator<Item = (PathBuf, PresetOrigin)> + '_ {
        let mine = std::iter::once((&self.data_home, PresetOrigin::EditedByYou));
        let system = self.system_data.iter().map(|d| (d, PresetOrigin::System));
        mine.chain(system).map(move |(d, origin)| {
            (
                d.join("quire").join("permissions").join(preset.folder()),
                origin,
            )
        })
    }

    /// The assistant's directory, inside the person's.
    pub fn assistant(&self) -> PathBuf {
        self.person.join("assistant")
    }

    /// The directory `preset` is read from, and whose it is: the first that holds a policy file.
    pub fn preset(&self, preset: Permissions) -> Option<(PathBuf, PresetOrigin)> {
        self.presets(preset)
            .find(|(d, _)| !cedar_files(d).is_empty())
    }

    /// The preset directories that exist, for the watch.
    pub fn preset_dirs(&self) -> Vec<PathBuf> {
        [Permissions::Auto, Permissions::Manual]
            .into_iter()
            .filter_map(|p| self.preset(p).map(|(dir, _)| dir))
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

/// Every source on disk for `preset`, with the built-in core. No preset on disk leaves the
/// built-in one.
pub fn read_sources(dirs: &PolicyDirs, preset: Permissions) -> Vec<PolicySource> {
    let folder = dirs
        .preset(preset)
        .map(|(dir, _)| read_layer(Layer::Preset, &dir))
        .unwrap_or_default();
    std::iter::once(PolicySource::core())
        .chain(folder)
        .chain(read_layer(Layer::Person, &dirs.person))
        .chain(read_layer(Layer::Assistant, &dirs.assistant()))
        .chain(read_layer(Layer::Admin, &dirs.admin))
        .collect()
}

/// The policy sets the files make now, with what was left out and where each preset came from.
#[derive(Debug)]
pub struct LoadedPolicies {
    /// One set per preset.
    pub presets: Presets,
    /// The sources left out, each once.
    pub skipped: Vec<Rejected>,
    /// Where the Auto preset in force came from.
    pub auto: PresetOrigin,
    /// Where the Manual preset in force came from.
    pub manual: PresetOrigin,
}

/// The origin a preset has once the loader has had its say: a folder that was skipped is the
/// built-in copy.
fn origin(dirs: &PolicyDirs, preset: Permissions, skipped: &[Rejected]) -> PresetOrigin {
    let dropped = skipped.iter().any(|r| r.layer == Layer::Preset);
    match dirs.preset(preset) {
        Some((_, origin)) if !dropped => origin,
        Some(_) | None => PresetOrigin::BuiltIn,
    }
}

/// The policy sets the files make now.
pub fn load_policies(dirs: &PolicyDirs) -> Result<LoadedPolicies, LayerError> {
    let auto = Pdp::layered_for(
        SCHEMA,
        Permissions::Auto,
        &read_sources(dirs, Permissions::Auto),
    )?;
    let manual = Pdp::layered_for(
        SCHEMA,
        Permissions::Manual,
        &read_sources(dirs, Permissions::Manual),
    )?;
    let auto_origin = origin(dirs, Permissions::Auto, &auto.skipped);
    let manual_origin = origin(dirs, Permissions::Manual, &manual.skipped);
    let mut skipped = auto.skipped;
    for rejected in manual.skipped {
        if !skipped.contains(&rejected) {
            skipped.push(rejected);
        }
    }
    Ok(LoadedPolicies {
        presets: Presets {
            auto: auto.pdp,
            manual: manual.pdp,
        },
        skipped,
        auto: auto_origin,
        manual: manual_origin,
    })
}
