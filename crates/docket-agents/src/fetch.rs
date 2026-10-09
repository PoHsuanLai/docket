//! The one place the network is reached. `Fetch` is the seam; the install and update commands
//! pass `Curl`, tests pass `Files` over a directory.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Why bytes did not arrive.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("could not download {0}")]
pub struct FetchFault(pub String);

/// The largest download docket takes, in bytes.
pub const MAX_BYTES: u64 = 1 << 30;

/// Gets the bytes at a location.
pub trait Fetch {
    /// The bytes at `url`.
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchFault>;
}

/// Downloads with the system's `curl`: https only, redirects followed, size capped.
#[derive(Debug, Clone, Copy)]
pub struct Curl;

impl Fetch for Curl {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchFault> {
        let fault = || FetchFault(url.to_owned());
        if !url.starts_with("https://") {
            return Err(fault());
        }
        let out = Command::new("curl")
            .args([
                "--fail",
                "--silent",
                "--location",
                "--proto",
                "=https",
                "--max-filesize",
            ])
            .arg(MAX_BYTES.to_string())
            .arg("--")
            .arg(url)
            .output()
            .map_err(|_| fault())?;
        out.status.success().then_some(out.stdout).ok_or_else(fault)
    }
}

/// Reads `file://NAME` from a directory: for tests and for archives the person already has.
#[derive(Debug, Clone)]
pub struct Files(PathBuf);

impl Files {
    /// Serves the files under `dir`.
    pub fn under(dir: &Path) -> Self {
        Self(dir.to_owned())
    }
}

impl Fetch for Files {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchFault> {
        let fault = || FetchFault(url.to_owned());
        let name = url.strip_prefix("file://").ok_or_else(fault)?;
        let plain = !name.is_empty() && !name.contains("..") && !name.starts_with('/');
        if !plain {
            return Err(fault());
        }
        std::fs::read(self.0.join(name)).map_err(|_| fault())
    }
}
