//! The administrator's floor file: `/etc/quire/docket-floor.toml`. A missing file is no floor. A
//! file that exists and does not read is an error that stops the daemon, never a silent skip: a
//! floor the daemon ignored would leave the machine looser than its administrator meant.
//!
//! The shapes and the rules are `docket_core`'s ([`Floor`]); this reads the file.

use docket_permissions_core::{Floor, FloorFile};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Where the floor is on a real machine.
pub const FLOOR_FILE: &str = "/etc/quire/docket-floor.toml";

/// Why the floor file could not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FloorError {
    /// The file exists and could not be read.
    #[error("cannot read {path}: {kind}")]
    Read {
        /// The file.
        path: PathBuf,
        /// What the system said.
        kind: ErrorKind,
    },
    /// The file is not a floor.
    #[error("{path} is not a floor: {why}")]
    Parse {
        /// The file.
        path: PathBuf,
        /// What the parser said.
        why: String,
    },
}

/// The floor the file at `path` describes: none when the file is missing.
pub fn read_floor(path: &Path) -> Result<Floor, FloorError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(why) if why.kind() == ErrorKind::NotFound => return Ok(Floor::none()),
        Err(why) => {
            return Err(FloorError::Read {
                path: path.to_owned(),
                kind: why.kind(),
            });
        }
    };
    let parse = |why: String| FloorError::Parse {
        path: path.to_owned(),
        why,
    };
    let file: FloorFile = toml::from_str(&text).map_err(|why| parse(why.to_string()))?;
    Floor::from_file(file).map_err(|why| parse(why.to_string()))
}
