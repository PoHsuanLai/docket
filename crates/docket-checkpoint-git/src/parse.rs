//! Reading what git prints. Pure.

use docket_checkpoint::{EntryId, Saved, TreeId, TreeListing};
use docket_core::{CheckpointId, WorkPath};
use porter_core::UnixSeconds;
use prov::SessionId;
use std::collections::BTreeMap;

use crate::names::id_of_ref;

/// The mode git gives a nested repository: it is not a file of this folder.
const NESTED_REPOSITORY: &str = "160000";

/// Output that is not what the command prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("git printed something unexpected")]
pub(crate) struct Unexpected;

fn lines(stdout: &[u8]) -> impl Iterator<Item = &[u8]> {
    stdout.split(|b| *b == 0).filter(|line| !line.is_empty())
}

fn text(bytes: &[u8]) -> Result<&str, Unexpected> {
    std::str::from_utf8(bytes).map_err(|_| Unexpected)
}

/// The number of paths in a NUL-separated listing.
pub(crate) fn count_paths(stdout: &[u8]) -> usize {
    lines(stdout).count()
}

/// `git ls-files -s -z`: `<mode> <oid> <stage>\t<path>`. A file's content id is its mode and
/// object id, so a change of the executable bit counts as a change.
pub(crate) fn staged_listing(stdout: &[u8]) -> Result<TreeListing, Unexpected> {
    let mut files = BTreeMap::new();
    for line in lines(stdout) {
        let line = text(line)?;
        let (head, path) = line.split_once('\t').ok_or(Unexpected)?;
        let mut fields = head.split(' ');
        let (Some(mode), Some(oid), Some(_stage), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            return Err(Unexpected);
        };
        add(&mut files, mode, oid, path)?;
    }
    Ok(TreeListing(files))
}

/// `git ls-tree -r -z`: `<mode> <type> <oid>\t<path>`.
pub(crate) fn tree_listing(stdout: &[u8]) -> Result<TreeListing, Unexpected> {
    let mut files = BTreeMap::new();
    for line in lines(stdout) {
        let line = text(line)?;
        let (head, path) = line.split_once('\t').ok_or(Unexpected)?;
        let mut fields = head.split(' ');
        let (Some(mode), Some(_kind), Some(oid), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            return Err(Unexpected);
        };
        add(&mut files, mode, oid, path)?;
    }
    Ok(TreeListing(files))
}

fn add(
    files: &mut BTreeMap<WorkPath, EntryId>,
    mode: &str,
    oid: &str,
    path: &str,
) -> Result<(), Unexpected> {
    if mode == NESTED_REPOSITORY {
        return Ok(());
    }
    let path = WorkPath::parse(path).map_err(|_| Unexpected)?;
    files.insert(path, EntryId::new(format!("{mode}:{oid}")));
    Ok(())
}

/// `git for-each-ref --format='%(refname)<TAB>%(tree)<TAB>%(committerdate:unix)'`, one point per
/// line; a ref that is not one of `session`'s points is skipped. Oldest first.
pub(crate) fn held_points(session: &SessionId, stdout: &[u8]) -> Vec<Saved> {
    let mut points: Vec<Saved> = String::from_utf8_lossy(stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let (refname, tree, at) = (fields.next()?, fields.next()?, fields.next()?);
            Some(Saved {
                id: id_of_ref(session, refname)?,
                at: UnixSeconds(at.trim().parse().ok()?),
                tree: TreeId::new(tree),
            })
        })
        .collect();
    points.sort_by_key(|saved| saved.id);
    points
}

/// The first line of a command's output as an object id.
pub(crate) fn object_id(stdout: &[u8]) -> Result<String, Unexpected> {
    let first = text(stdout)?.lines().next().map(str::trim).unwrap_or("");
    if first.is_empty() || !first.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Unexpected);
    }
    Ok(first.to_owned())
}

/// The ids of the points of `held` that `ids` names.
pub(crate) fn named(held: &[Saved], ids: &[CheckpointId]) -> Vec<CheckpointId> {
    held.iter()
        .map(|saved| saved.id)
        .filter(|id| ids.contains(id))
        .collect()
}
