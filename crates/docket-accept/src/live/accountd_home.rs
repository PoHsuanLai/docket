//! A durable, test-only home for the cloud account: one directory outside every repository that
//! holds what accountd needs to know the OpenRouter account across worlds, so the owner types the
//! key once (`dev/live/cloud-key.sh`) and every fresh world reuses it.
//!
//! ```text
//! DIR/                      0700
//!   accountd.keys           0600  the key file; only accountd opens it (ACCOUNTD_KEYS=file:...)
//!   accountd.keys.lock            accountd's advisory lock
//!   state/porter/...              the registry and grants accountd wrote (no secrets)
//!   data/porter/providers/        the provider files `add` used
//! ```
//!
//! This process never opens the key file: it only looks at its metadata. A world points accountd's
//! `ACCOUNTD_KEYS` at it and copies the non-secret `state` and `data` trees into its scratch
//! root, so a run never changes the home.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// The key file's name inside the home.
pub const KEYS_FILE: &str = "accountd.keys";
/// The registry accountd keeps, relative to the home.
const REGISTRY: &str = "state/porter/registry.json";

/// Why a directory cannot be the home.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HomeFault {
    /// The path is not absolute.
    #[error("--accountd-home {0} is not an absolute path")]
    NotAbsolute(PathBuf),
    /// The directory is missing or is not one.
    #[error("--accountd-home {0} is not a directory")]
    NotADirectory(PathBuf),
    /// Group or other can use the directory or the key file.
    #[error("{0} has group or other permission bits ({1:o}); chmod it to 0700 (the key file 0600)")]
    LooseMode(PathBuf, u32),
    /// The directory is inside a git work tree.
    #[error(
        "--accountd-home {0} is inside a git work tree ({1}); keys must live outside every repository"
    )]
    InsideGit(PathBuf, PathBuf),
    /// The key file is not a plain file.
    #[error("{0} is not a regular file")]
    NotAFile(PathBuf),
    /// The home does not hold the account yet.
    #[error(
        "{home} does not hold the cloud account yet. Fill it once, in your own terminal (the key is typed at accountd's own prompt, echo off):\n  dev/live/cloud-key.sh {home} {accountd}\nthen run again."
    )]
    Empty {
        /// The directory.
        home: PathBuf,
        /// The accountd binary the run was given.
        accountd: PathBuf,
    },
    /// The directory could not be read.
    #[error("{0}: {1}")]
    Io(PathBuf, String),
}

/// What a world needs to start accountd against a home: the test build and the checked home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cloud {
    /// The accountd binary.
    pub accountd: PathBuf,
    /// The home that holds the account.
    pub home: AccountdHome,
}

/// A checked home that holds the account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountdHome(PathBuf);

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

impl AccountdHome {
    /// Checks `dir` and that it holds the account. `accountd` only names the binary in the
    /// message that tells the owner how to fill an empty home.
    pub fn open(dir: &Path, accountd: &Path) -> Result<Self, HomeFault> {
        let real = check_dir(dir)?;
        let keys = real.join(KEYS_FILE);
        let empty = || HomeFault::Empty {
            home: dir.to_owned(),
            accountd: accountd.to_owned(),
        };
        // Metadata only: the harness never opens the key file.
        let meta = std::fs::symlink_metadata(&keys).map_err(|_| empty())?;
        if !meta.is_file() {
            return Err(HomeFault::NotAFile(keys));
        }
        tight(&keys, &meta)?;
        std::fs::metadata(real.join(REGISTRY)).map_err(|_| empty())?;
        Ok(Self(real))
    }

    /// The value for `ACCOUNTD_KEYS`.
    pub fn keys_setting(&self) -> String {
        format!("file:{}", self.0.join(KEYS_FILE).display())
    }

    /// Copies the non-secret records into a world's scratch root: accountd's state under
    /// `.local/state` and its provider files under `data/porter/providers`.
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
