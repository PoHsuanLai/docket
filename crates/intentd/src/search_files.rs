//! The entries apps ship: `quire/search/<AppName>.toml` under the data directories, read at
//! start with the same trust as the manifest beside it. Same order as the manifests: an earlier
//! directory wins for the same app. A file of an app with no manifest is ignored, and one that
//! is not a search file is said so and ignored.

use crate::shipped::{ShippedEntries, ShippedFault};
use docket_router::{Router, Seams};
use porter_core::AppName;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The directory a data directory keeps search files in.
pub fn search_dir(data: &Path) -> PathBuf {
    data.join("quire").join("search")
}

/// Installs the entries of every search file under `data_dirs`, first app wins.
pub fn ship_search_files<S: Seams>(router: &Router<S>, data_dirs: &[PathBuf]) {
    let mut seen = BTreeSet::new();
    for dir in data_dirs.iter().map(|d| search_dir(d)) {
        let Ok(files) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = files
            .filter_map(Result::ok)
            .map(|f| f.path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect();
        files.sort();
        for file in files {
            let app = file
                .file_stem()
                .and_then(|n| n.to_str())
                .and_then(|n| AppName::parse(n).ok());
            let Some(app) = app else {
                continue;
            };
            if !router.has_manifest(&app) || !seen.insert(app.clone()) {
                continue;
            }
            match read(&file) {
                Ok(entries) => router.ship_index(&app, entries.into_entries()),
                Err(why) => eprintln!("intentd: search file {} skipped: {why}", file.display()),
            }
        }
    }
}

/// Why a search file was not taken.
#[derive(Debug, thiserror::Error)]
enum SearchFileFault {
    #[error("{0}")]
    Unread(#[from] std::io::Error),
    #[error("{0}")]
    Unshipped(#[from] ShippedFault),
}

fn read(file: &Path) -> Result<ShippedEntries, SearchFileFault> {
    let text = std::fs::read_to_string(file)?;
    Ok(ShippedEntries::parse(&text)?)
}
