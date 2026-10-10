//! The person's standing consent in a file the app names, so "always" survives a restart.
//!
//! The format is intentd's own (`grants.json`: a JSON list of grants), so a file written here is
//! one the desktop's `FileGrants` reads. Nothing here finds a directory: the app gives the path
//! (its own data directory), and no `/proc`, XDG lookup or platform call is made. A write goes
//! through a temporary file beside the target and a rename, so a crash leaves the old file or
//! the new one, never half of one. The `GrantStore` seam has no way to say "failed" (the router
//! must not stop a call over a disk), so a failed write is kept for the app to ask for
//! ([`FileGrantStore::take_fault`]); the grant still counts for this run.

use docket_core::{
    ActionGrant, GrantFileError, Revocation, StandingGrant, StandingGrantId, decode_standing,
    encode_standing, held_with, held_without, read_grant_list, write_grant_list,
};
use docket_router::GrantStore;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// The grants a file's text holds.
fn decode(text: &str) -> Result<Vec<ActionGrant>, serde_json::Error> {
    serde_json::from_str(text)
}

/// The text of a list of grants. An error is never turned into an empty list: that would
/// overwrite every grant held.
fn encode(grants: &[ActionGrant]) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(grants)
}

/// `grants` with `grant` recorded: a grant for the same key replaces the earlier one.
fn recorded(mut grants: Vec<ActionGrant>, grant: ActionGrant) -> Vec<ActionGrant> {
    grants.retain(|held| held.key != grant.key);
    grants.push(grant);
    grants
}

fn read_file(path: &Path) -> Result<Vec<ActionGrant>, GrantFileError> {
    read_grant_list(path, decode)
}

fn write_file(path: &Path, grants: &[ActionGrant]) -> Result<(), GrantFileError> {
    write_grant_list(path, encode(grants))
}

#[derive(Debug, Default)]
struct Held {
    grants: Vec<ActionGrant>,
    fault: Option<GrantFileError>,
}

/// Grants kept in the file at the path the app gave. Two stores on one file see each other's
/// grants: a read goes to the file, and a record merges into what the file holds.
#[derive(Debug)]
pub struct FileGrantStore {
    path: PathBuf,
    held: Mutex<Held>,
    on_damage: OnDamage,
}

/// What a write does when the file it would merge into cannot be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnDamage {
    /// Leave the file alone and keep the fault for the app: the grants it holds may be the
    /// person's only copy.
    Keep,
    /// Replace it with what this run holds (the app chose to start over).
    Replace,
}

impl FileGrantStore {
    /// The store for `path`, with what the file holds now (nothing when it is missing).
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, GrantFileError> {
        let path = path.into();
        let grants = read_file(&path)?;
        Ok(Self {
            path,
            held: Mutex::new(Held {
                grants,
                fault: None,
            }),
            on_damage: OnDamage::Keep,
        })
    }

    /// The store for `path` that does not fail on a damaged file: it holds nothing while the file
    /// is unreadable, and the first grant recorded replaces it.
    pub fn fresh(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            held: Mutex::default(),
            on_damage: OnDamage::Replace,
        }
    }

    /// The file this store keeps.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The last write that failed, if one did; asking clears it.
    pub fn take_fault(&self) -> Option<GrantFileError> {
        self.locked().fault.take()
    }

    /// What a write merges into: the file's list, or when it cannot be read, this run's
    /// `fallback` for a store that started over, and the fault for one that did not.
    fn base<T>(
        &self,
        read: Result<Vec<T>, GrantFileError>,
        fallback: Vec<T>,
    ) -> Result<Vec<T>, GrantFileError> {
        match (read, self.on_damage) {
            (Ok(on_disk), _) => Ok(on_disk),
            (Err(_), OnDamage::Replace) => Ok(fallback),
            (Err(why), OnDamage::Keep) => Err(why),
        }
    }

    fn locked(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl GrantStore for FileGrantStore {
    fn grants(&self) -> Vec<ActionGrant> {
        let mut held = self.locked();
        if let Ok(on_disk) = read_file(&self.path) {
            held.grants = on_disk;
        }
        held.grants.clone()
    }

    fn record(&self, grant: ActionGrant) {
        let mut held = self.locked();
        let kept = held.grants.clone();
        let base = self.base(read_file(&self.path), kept.clone());
        // The grant counts for this run whatever the file does.
        held.grants = recorded(base.clone().unwrap_or(kept), grant);
        held.fault = base
            .and_then(|_| write_file(&self.path, &held.grants))
            .err();
    }

    // Standing grants live in the file beside the grants, read afresh on every call so a
    // revocation made by another process takes effect on the next call. An unreadable file holds
    // none (the person is asked again) and is not written over.
    fn standing(&self) -> Vec<StandingGrant> {
        read_standing(&self.standing_file()).unwrap_or_default()
    }

    fn add_standing(&self, grant: StandingGrant) {
        let mut held = self.locked();
        let file = self.standing_file();
        held.fault = self
            .base(read_standing(&file), Vec::new())
            .and_then(|current| write_standing(&file, &held_with(current, grant)))
            .err();
    }

    fn revoke_standing(&self, id: &StandingGrantId) -> Revocation {
        let mut held = self.locked();
        let file = self.standing_file();
        match self.base(read_standing(&file), Vec::new()) {
            Err(why) => {
                held.fault = Some(why);
                Revocation::NotHeld
            }
            Ok(current) => {
                let (rest, done) = held_without(current, id);
                if done == Revocation::Revoked {
                    held.fault = write_standing(&file, &rest).err();
                }
                done
            }
        }
    }
}

/// The file beside `path` that holds the standing grants: the same name and `.standing`.
fn standing_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".standing");
    path.with_file_name(name)
}

fn read_standing(path: &Path) -> Result<Vec<StandingGrant>, GrantFileError> {
    read_grant_list(path, decode_standing)
}

fn write_standing(path: &Path, grants: &[StandingGrant]) -> Result<(), GrantFileError> {
    write_grant_list(path, encode_standing(grants))
}

impl FileGrantStore {
    fn standing_file(&self) -> PathBuf {
        standing_path(&self.path)
    }
}
