//! The person's standing consent for actions, in a file under the user's data directory.

use docket_core::ActionGrant;
use docket_router::GrantStore;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One writer at a time: a grant recorded while another is being written is added to the file
/// the first left, not to the one it read before.
static WRITING: Mutex<()> = Mutex::new(());

/// Grants kept in `$XDG_DATA_HOME/quire/intents/grants.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileGrants {
    path: PathBuf,
}

impl FileGrants {
    /// Grants stored at `path`.
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// What the file holds: nothing when it is missing, and nothing (with a line on standard
    /// error) when it is not a list of grants. A damaged file never takes the daemon down; it
    /// only means the person is asked again.
    fn read(&self) -> Vec<ActionGrant> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|why| {
                eprintln!(
                    "intentd: {} is not a list of grants: {why}",
                    self.path.display()
                );
                Vec::new()
            }),
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(why) => {
                eprintln!("intentd: cannot read {}: {why}", self.path.display());
                Vec::new()
            }
        }
    }

    /// Writes `grants` through a temporary file in the same directory and renames it into
    /// place, so a crash leaves the old file or the new one, never half of one.
    fn write(&self, grants: &[ActionGrant]) -> std::io::Result<()> {
        let dir = self.path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)?;
        let temporary = self.path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(grants)?;
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&temporary)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
        }
        std::fs::rename(&temporary, &self.path)
    }
}

impl GrantStore for FileGrants {
    fn grants(&self) -> Vec<ActionGrant> {
        self.read()
    }

    fn record(&self, grant: ActionGrant) {
        let _one_at_a_time = WRITING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut all = self.read();
        all.retain(|held| held.key != grant.key);
        all.push(grant);
        if let Err(why) = self.write(&all) {
            eprintln!("intentd: cannot write {}: {why}", self.path.display());
        }
    }
}
