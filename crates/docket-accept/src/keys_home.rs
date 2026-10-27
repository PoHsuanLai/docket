//! A durable, test-only home for the cloud AI key: one directory outside every repository that
//! holds what inferd needs to know the OpenRouter account across worlds, so the owner types the
//! key once (`dev/live/cloud-key.sh`) and every fresh world reuses it.
//!
//! ```text
//! DIR/                           0700
//!   inferd.keys                  0600  the key file; only inferd opens it (INFERD_KEYS=file:...)
//!   inferd.keys.lock                   the key file's advisory lock
//!   state/quire/inferd/keys/...        the registry (accounts and key permits; no secrets)
//!   data/porter/providers/             the provider files `inferd add` used
//! ```
//!
//! This process never opens the key file: it only looks at its metadata. A world points inferd's
//! `INFERD_KEYS` at it and copies the non-secret `state` and `data` trees into its scratch
//! root, so a run never changes the home.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// The key file's name inside the home.
pub const KEYS_FILE: &str = "inferd.keys";
/// The registry inferd keeps (accounts and key permits), relative to the home.
const REGISTRY: &str = "state/quire/inferd/keys/registry.json";

/// Why a directory cannot be the home.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HomeFault {
    /// The path is not absolute.
    #[error("--keys-home {0} is not an absolute path")]
    NotAbsolute(PathBuf),
    /// The directory is missing or is not one.
    #[error("--keys-home {0} is not a directory")]
    NotADirectory(PathBuf),
    /// Group or other can use the directory or the key file.
    #[error("{0} has group or other permission bits ({1:o}); chmod it to 0700 (the key file 0600)")]
    LooseMode(PathBuf, u32),
    /// The directory is inside a git work tree.
    #[error(
        "--keys-home {0} is inside a git work tree ({1}); keys must live outside every repository"
    )]
    InsideGit(PathBuf, PathBuf),
    /// The key file is not a plain file.
    #[error("{0} is not a regular file")]
    NotAFile(PathBuf),
    /// The home does not hold the account yet.
    #[error(
        "{0} does not hold the cloud key yet. Fill it once, in your own terminal (the key is typed at inferd's own prompt, echo off):\n  dev/live/cloud-key.sh {0} <inferd built with --features test-keys>\nthen run again."
    )]
    Empty(PathBuf),
    /// The directory could not be read.
    #[error("{0}: {1}")]
    Io(PathBuf, String),
}

/// A checked home that holds the cloud key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeysHome(PathBuf);

fn meta_of(path: &Path) -> Result<std::fs::Metadata, HomeFault> {
    std::fs::symlink_metadata(path).map_err(|e| HomeFault::Io(path.to_owned(), e.to_string()))
}

fn tight(path: &Path, meta: &std::fs::Metadata) -> Result<(), HomeFault> {
    let mode = meta.permissions().mode() & 0o777;
    match mode & 0o077 {
        0 => Ok(()),
        _ => Err(HomeFault::LooseMode(path.to_owned(), mode)),
    }
}

/// The nearest ancestor (or the directory itself) that holds a `.git`.
fn git_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|p| p.join(".git").exists())
        .map(Path::to_owned)
}

/// Checks a directory the way `open` does: absolute, a directory, no group or other bits, outside
/// every git work tree. Returns its real path.
pub fn check_dir(dir: &Path) -> Result<PathBuf, HomeFault> {
    if !dir.is_absolute() {
        return Err(HomeFault::NotAbsolute(dir.to_owned()));
    }
    let meta = meta_of(dir).map_err(|_| HomeFault::NotADirectory(dir.to_owned()))?;
    if !meta.is_dir() {
        return Err(HomeFault::NotADirectory(dir.to_owned()));
    }
    tight(dir, &meta)?;
    let real =
        std::fs::canonicalize(dir).map_err(|e| HomeFault::Io(dir.to_owned(), e.to_string()))?;
    match git_root(&real) {
        Some(root) => Err(HomeFault::InsideGit(dir.to_owned(), root)),
        None => Ok(real),
    }
}

impl KeysHome {
    /// Checks `dir` and that it holds the account.
    pub fn open(dir: &Path) -> Result<Self, HomeFault> {
        let real = check_dir(dir)?;
        let keys = real.join(KEYS_FILE);
        let empty = || HomeFault::Empty(dir.to_owned());
        // Metadata only: the harness never opens the key file.
        let meta = std::fs::symlink_metadata(&keys).map_err(|_| empty())?;
        if !meta.is_file() {
            return Err(HomeFault::NotAFile(keys));
        }
        tight(&keys, &meta)?;
        std::fs::metadata(real.join(REGISTRY)).map_err(|_| empty())?;
        Ok(Self(real))
    }

    /// The value for `INFERD_KEYS`.
    pub fn keys_setting(&self) -> String {
        format!("file:{}", self.0.join(KEYS_FILE).display())
    }

    /// Copies the non-secret records into a world's scratch root: inferd's state under
    /// `.local/state` and the provider files under `data/porter/providers`.
    pub fn seed(&self, root: &Path) -> std::io::Result<()> {
        copy_tree(&self.0.join("state"), &root.join(".local/state"))?;
        copy_tree(&self.0.join("data"), &root.join("data"))
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    if !from.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if entry.file_type()?.is_file() {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
