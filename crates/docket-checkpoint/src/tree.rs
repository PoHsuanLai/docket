//! A folder's files as a map from path to content id, and the plan that turns one into the other.

use crate::ids::EntryId;
use docket_core::{PlanDigest, RestorePlan, WorkPath};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// The files of a folder (or of a saved point): path to content id. Files the folder ignores are
/// never in one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TreeListing(pub BTreeMap<WorkPath, EntryId>);

/// What restoring `then` changes in the folder as it is `now`:
///
/// - `changed`: in both, content differs;
/// - `added`: only in `then`, so brought back;
/// - `removed`: only in `now`, so deleted.
///
/// Each list is sorted by path. The digest is SHA-256 (lower-case hex) over the three lists, each
/// path with the content ids the restore would meet: what the file holds now and what it would
/// hold then. It stands in for the point's tree id (a listing carries none), and it binds a
/// confirmation to exactly what the person saw, down to the content: a file edited since the
/// plan was shown makes the plan stale.
pub fn plan_restore(now: &TreeListing, then: &TreeListing) -> RestorePlan {
    let mut changed = Vec::new();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    for (path, id) in &then.0 {
        match now.0.get(path) {
            Some(current) if current == id => {}
            Some(_) => changed.push(path.clone()),
            None => added.push(path.clone()),
        }
    }
    for path in now.0.keys() {
        if !then.0.contains_key(path) {
            removed.push(path.clone());
        }
    }
    let digest = digest_of(now, then, &[&changed, &added, &removed]);
    RestorePlan {
        changed,
        added,
        removed,
        digest,
    }
}

fn digest_of(now: &TreeListing, then: &TreeListing, lists: &[&Vec<WorkPath>; 3]) -> PlanDigest {
    let mut hash = Sha256::new();
    hash.update(b"docket-restore-plan-v1");
    for list in lists {
        hash.update(b"\x00list");
        feed(&mut hash, list.len().to_string().as_bytes());
        for path in list.iter() {
            feed(&mut hash, path.as_str().as_bytes());
            feed(&mut hash, id_of(now, path));
            feed(&mut hash, id_of(then, path));
        }
    }
    let sum = hash.finalize();
    PlanDigest(sum.iter().map(|b| format!("{b:02x}")).collect())
}

fn id_of<'a>(listing: &'a TreeListing, path: &WorkPath) -> &'a [u8] {
    match listing.0.get(path) {
        Some(id) => id.as_str().as_bytes(),
        None => &[],
    }
}

/// A field with its length in front, so no two different plans feed the same bytes.
fn feed(hash: &mut Sha256, field: &[u8]) {
    hash.update(field.len().to_string().as_bytes());
    hash.update(b":");
    hash.update(field);
}
