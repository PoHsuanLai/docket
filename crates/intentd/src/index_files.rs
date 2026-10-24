//! What apps pushed to the index, kept across logins: `quire/intents/index/<AppName>.json`
//! under the person's data directory, one file per app, `{"version":1,"entries":[...]}`. Each
//! write replaces the file whole (a temporary file in the same directory, mode 0600, synced,
//! renamed), so a crash leaves the old file or the new one. The entries are not encrypted: they
//! are as exposed as the app's own store.

use docket_core::IndexEntry;
use docket_router::{Router, Seams};
use porter_core::AppName;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The version of the file this build writes and reads.
const VERSION: u32 = 1;

/// Why an index file could not be written or read.
#[derive(Debug, thiserror::Error)]
pub enum IndexFileFault {
    /// The disk refused.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// The text is not the JSON of a file of this version.
    #[error("not an index file: {0}")]
    Malformed(String),
}

/// The file as it is on disk.
#[derive(Debug, Serialize, Deserialize)]
struct IndexFile {
    version: u32,
    entries: Vec<IndexEntry>,
}

/// The directory of index files.
#[derive(Debug, Clone)]
pub struct IndexStore {
    dir: PathBuf,
    /// Held while a file is written, so two writes of one app never share the temporary file.
    writing: Arc<Mutex<()>>,
}

impl IndexStore {
    /// The store under a data directory: `quire/intents/index`.
    pub fn under(data: &Path) -> Self {
        Self {
            dir: crate::manifests::intents_dir(data).join("index"),
            writing: Arc::default(),
        }
    }

    fn file_of(&self, app: &AppName) -> PathBuf {
        self.dir.join(format!("{app}.json"))
    }

    /// Replaces the file of `app` with `entries`.
    pub fn write(&self, app: &AppName, entries: Vec<IndexEntry>) -> Result<(), IndexFileFault> {
        let bytes = serde_json::to_vec(&IndexFile {
            version: VERSION,
            entries,
        })
        .map_err(|e| IndexFileFault::Malformed(e.to_string()))?;
        std::fs::create_dir_all(&self.dir)?;
        let target = self.file_of(app);
        let temporary = self.dir.join(format!("{app}.json.tmp"));
        let placed = write_private(&temporary, &bytes).and_then(|()| {
            std::fs::rename(&temporary, &target)?;
            std::fs::File::open(&self.dir)?.sync_all()
        });
        if placed.is_err() {
            // Best effort: the error that matters is the one being returned.
            let _ = std::fs::remove_file(&temporary);
        }
        Ok(placed?)
    }

    /// Reads the file at `path`.
    fn read(path: &Path) -> Result<Vec<IndexEntry>, IndexFileFault> {
        let file: IndexFile = serde_json::from_slice(&std::fs::read(path)?)
            .map_err(|e| IndexFileFault::Malformed(e.to_string()))?;
        match file.version {
            VERSION => Ok(file.entries),
            other => Err(IndexFileFault::Malformed(format!("version {other}"))),
        }
    }

    /// Writes the entries `app` has pushed to `router` now. Done outside the router's lock.
    pub fn save<S: Seams>(&self, router: &Router<S>, app: &AppName) {
        let _held = self.writing.lock().unwrap_or_else(|p| p.into_inner());
        if let Err(why) = self.write(app, router.pushed_entries(app)) {
            eprintln!("intentd: the index of {app} was not kept: {why}");
        }
    }

    /// Brings back the entries of every file here whose app has a manifest in `router`; a file of
    /// an app with none is deleted; one that cannot be read is said so and left.
    pub fn restore<S: Seams>(&self, router: &Router<S>) {
        let Ok(files) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for path in files.filter_map(Result::ok).map(|f| f.path()) {
            let app = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".json"))
                .and_then(|n| AppName::parse(n).ok());
            let Some(app) = app else {
                continue;
            };
            if !router.has_manifest(&app) {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            match Self::read(&path) {
                Ok(entries) => router.restore_index(&app, entries),
                Err(why) => eprintln!("intentd: {} not read: {why}", path.display()),
            }
        }
    }
}

/// Writes `bytes` to a new file only the person can read, and syncs it.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}
