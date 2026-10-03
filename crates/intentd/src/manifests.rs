//! The installed manifests: `quire/intents/<AppName>.toml` under the data directories. An
//! earlier directory wins over a later one for the same app (the person's own over the
//! system's), and a file that is not a manifest is skipped with its reason, never fatal: one
//! broken app must not take every other app's actions away.

use crate::builtin::is_builtin;
use docket_core::ValidManifest;
use docket_router::parse;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// What a scan found.
#[derive(Debug, Clone, Default)]
pub struct Loaded {
    /// The manifests, in the order found.
    pub manifests: Vec<ValidManifest>,
    /// The files skipped, and why.
    pub skipped: Vec<(PathBuf, String)>,
}

/// The directory a data directory keeps manifests in.
pub fn intents_dir(data: &Path) -> PathBuf {
    data.join("quire").join("intents")
}

/// Reads every `*.toml` of `quire/intents` under each of `data_dirs`, in order, first app wins.
pub fn load_manifests(data_dirs: &[PathBuf]) -> Loaded {
    let mut loaded = Loaded::default();
    let mut seen = BTreeSet::new();
    for dir in data_dirs.iter().map(|d| intents_dir(d)) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect();
        files.sort();
        for file in files {
            let text = match std::fs::read_to_string(&file) {
                Ok(text) => text,
                Err(why) => {
                    loaded.skipped.push((file, why.to_string()));
                    continue;
                }
            };
            match parse(&text) {
                // The built-in providers' declarations are intentd's own.
                Ok(manifest) if is_builtin(&manifest.manifest().app) => {
                    loaded
                        .skipped
                        .push((file, "that app is built into intentd".to_owned()));
                }
                Ok(manifest) if seen.insert(manifest.manifest().app.clone()) => {
                    loaded.manifests.push(manifest);
                }
                Ok(_) => {}
                Err(why) => loaded.skipped.push((file, why.to_string())),
            }
        }
    }
    loaded
}
