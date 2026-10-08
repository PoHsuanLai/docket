//! The person's standing consent for actions, in a file under the user's data directory.

use crate::defaults::{default_grants_file, read_defaults};
use crate::grant_file::{GrantFileFault, read_list, write_list};
use docket_core::{
    ActionGrant, Revocation, StandingGrant, StandingGrantId, decode_standing, encode_standing,
    held_with, held_without,
};
use docket_router::GrantStore;
use std::path::PathBuf;
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

    /// What the file holds: nothing when it is missing. A file that cannot be read or is not a
    /// list of grants is a fault, never an empty list.
    fn read(&self) -> Result<Vec<ActionGrant>, GrantFileFault> {
        read_list(&self.path, |text| serde_json::from_str(text))
    }

    fn standing_path(&self) -> PathBuf {
        self.path.with_file_name("standing.json")
    }

    fn read_standing(&self) -> Result<Vec<StandingGrant>, GrantFileFault> {
        read_list(&self.standing_path(), decode_standing)
    }

    fn write_standing(&self, grants: &[StandingGrant]) -> Result<(), GrantFileFault> {
        write_list(&self.standing_path(), encode_standing(grants))
    }

    fn write(&self, grants: &[ActionGrant]) -> Result<(), GrantFileFault> {
        write_list(&self.path, serde_json::to_string_pretty(grants))
    }
}

/// What a store does with a fault: the person is asked again (no grants held), and the line says
/// why. The damaged file is left alone for the person to look at.
fn logged<T>(done: Result<T, GrantFileFault>) -> Option<T> {
    done.map_err(|why| eprintln!("intentd: {why}")).ok()
}

impl GrantStore for FileGrants {
    fn grants(&self) -> Vec<ActionGrant> {
        let mut all: Vec<ActionGrant> = self
            .defaults
            .iter()
            .flat_map(|f| read_defaults(f))
            .collect();
        all.extend(logged(self.read()).unwrap_or_default());
        all
    }

    fn record(&self, grant: ActionGrant) {
        let _one_at_a_time = WRITING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // A file that cannot be read is not written over: the grant is not kept, and the
        // person is asked again.
        let Some(mut all) = logged(self.read()) else {
            return;
        };
        all.retain(|held| held.key != grant.key);
        all.push(grant);
        logged(self.write(&all));
    }

    // Standing grants ("allow always", scoped) in `standing.json` beside the grants file, read
    // afresh on every call: a revocation takes effect on the next call, and a damaged file holds
    // none (which only means the person is asked again) and is never written over.
    fn standing(&self) -> Vec<StandingGrant> {
        logged(self.read_standing()).unwrap_or_default()
    }

    fn add_standing(&self, grant: StandingGrant) {
        let _one_at_a_time = WRITING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(held) = logged(self.read_standing()) {
            logged(self.write_standing(&held_with(held, grant)));
        }
    }

    fn revoke_standing(&self, id: &StandingGrantId) -> Revocation {
        let _one_at_a_time = WRITING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(held) = logged(self.read_standing()) else {
            return Revocation::NotHeld;
        };
        let (rest, done) = held_without(held, id);
        if done == Revocation::Revoked {
            logged(self.write_standing(&rest));
        }
        done
    }
}
