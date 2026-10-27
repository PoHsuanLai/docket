//! What to run for an installed agent. The launcher turns it into an `agents.toml` entry. This is
//! the value; reading the record and making the home is the caller's.

use crate::record::{FILES, Record, RecordFault};
use crate::slug::SlugRefused;
use std::path::{Path, PathBuf};

/// How to start an installed agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// The program, absolute.
    pub command: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// Plain variables it wants.
    pub env: Vec<(String, String)>,
    /// Where it is installed (read-only to it).
    pub files: PathBuf,
    /// Its own home, read-write, kept between runs: its sign-in lives here.
    pub home: PathBuf,
}

/// Why an agent cannot be started from the agents directory.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LaunchFault {
    /// The id or the version is not a name docket keeps agents under.
    #[error(transparent)]
    Name(#[from] SlugRefused),
    /// Not installed, or the record is damaged.
    #[error("{id} {version} is not installed ({why})")]
    Record {
        /// The id.
        id: String,
        /// The pinned version.
        version: String,
        /// What is wrong.
        why: RecordFault,
    },
    /// The home could not be made.
    #[error("the agent's home could not be made")]
    Home,
}

impl Launch {
    /// How to start the agent whose record is `record`, installed at `dir` (the version's
    /// directory) with `home` as its home.
    pub fn of(record: &Record, dir: &Path, home: PathBuf) -> Self {
        Self {
            command: record.command_in(dir),
            args: record.args.clone(),
            env: record.env.clone().into_iter().collect(),
            files: dir.join(FILES),
            home,
        }
    }
}
