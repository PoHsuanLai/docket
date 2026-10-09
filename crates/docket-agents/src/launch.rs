//! What to run for an installed agent. The launcher turns it into an `agents.toml` entry.

use crate::dirs::AgentsDir;
use crate::record::{FILES, Record, RecordFault};
use crate::slug::{Slug, SlugRefused};
use std::path::PathBuf;

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

impl AgentsDir {
    /// How to start `id` at `version`; makes its home when it is not there yet.
    pub fn launch(&self, id: &str, version: &str) -> Result<Launch, LaunchFault> {
        let (slug, pinned) = (Slug::parse(id)?, Slug::parse(version)?);
        let dir = self.installed(&slug, &pinned);
        let record = Record::read(&dir).map_err(|why| LaunchFault::Record {
            id: id.to_owned(),
            version: version.to_owned(),
            why,
        })?;
        let home = self.home(&slug);
        std::fs::create_dir_all(&home).map_err(|_| LaunchFault::Home)?;
        Ok(Launch {
            command: record.command_in(&dir),
            args: record.args.clone(),
            env: record.env.clone().into_iter().collect(),
            files: dir.join(FILES),
            home,
        })
    }
}
