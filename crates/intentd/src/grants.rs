//! The person's standing consent for actions, in a file under the user's data directory.

use crate::defaults::{default_grants_file, read_defaults};
use docket_core::{
    ActionGrant, Revocation, StandingGrant, StandingGrantId, decode_standing, encode_standing,
    held_with, held_without,
};
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
    /// The shipped defaults files, read-only, layered under what the person's file holds.
    defaults: Vec<PathBuf>,
}

impl FileGrants {
    /// Grants stored at `path`.
    pub fn at(path: PathBuf) -> Self {
        Self {
            path,
            defaults: Vec::new(),
        }
    }

    /// The same store with the shipped defaults of each data directory under it
    /// (`quire/intents/default-grants.json`): the person's own entries, a denial included,
    /// always win over a default.
    pub fn with_defaults(self, data_dirs: &[PathBuf]) -> Self {
        Self {
            defaults: data_dirs.iter().map(|d| default_grants_file(d)).collect(),
            ..self
        }
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

    fn standing_path(&self) -> PathBuf {
        self.path.with_file_name("standing.json")
    }

    fn read_standing(&self) -> Vec<StandingGrant> {
        let path = self.standing_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => decode_standing(&text).unwrap_or_else(|why| {
                eprintln!("intentd: {} is not a list of grants: {why}", path.display());
                Vec::new()
            }),
            Err(_) => Vec::new(),
        }
    }

    fn write_standing(&self, grants: &[StandingGrant]) {
        let path = self.standing_path();
        let temporary = path.with_extension("json.tmp");
        let done = std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))
            .and_then(|()| std::fs::write(&temporary, encode_standing(grants)))
            .and_then(|()| std::fs::rename(&temporary, &path));
        if let Err(why) = done {
            eprintln!("intentd: cannot write {}: {why}", path.display());
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
        let mut all: Vec<ActionGrant> = self
            .defaults
            .iter()
            .flat_map(|f| read_defaults(f))
            .collect();
        all.extend(self.read());
        all
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

    // Standing grants ("allow always", scoped) in `standing.json` beside the grants file, read
    // afresh on every call: a revocation takes effect on the next call, and a damaged file holds
    // none, which only means the person is asked again.
    fn standing(&self) -> Vec<StandingGrant> {
        self.read_standing()
    }

    fn add_standing(&self, grant: StandingGrant) {
        let _one_at_a_time = WRITING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.write_standing(&held_with(self.read_standing(), grant));
    }

    fn revoke_standing(&self, id: &StandingGrantId) -> Revocation {
        let _one_at_a_time = WRITING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (rest, done) = held_without(self.read_standing(), id);
        if done == Revocation::Revoked {
            self.write_standing(&rest);
        }
        done
    }
}
