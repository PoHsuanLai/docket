//! Consent keyed by Space: what a grant file holds when a Space is gone, was written before
//! Spaces had ids of porter's making, or sits over another app's own Space. Every outcome here
//! only removes grants. A grant that cannot be placed in a Space porter knows is dropped, never
//! moved to another Space and never widened to every Space.

use crate::grant::{ActionGrant, ActionGrantKey};
use crate::space_access::{SpaceAccess, SpaceRefusal};
use porter_core::{DesktopSpace, SpaceKind};
use prov::{SpaceId, SpaceScope};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Why grants ended without the person ending them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum GrantEnd {
    /// The Space is gone, or porter does not know it.
    SpaceGone(SpaceId),
    /// The Space the grant was written for is not an id porter reads.
    SpaceUnreadable,
    /// The grant was for an app over another app's own Space.
    NotTheirSpace(SpaceId),
}

/// How many grants ended for one reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ended {
    /// Why.
    pub why: GrantEnd,
    /// How many.
    pub count: u32,
}

/// The desktop-wide Spaces porter's registry holds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KnownSpaces(BTreeSet<SpaceId>);

impl KnownSpaces {
    /// The registry's Spaces.
    pub fn of(spaces: impl IntoIterator<Item = DesktopSpace>) -> Self {
        Self(spaces.into_iter().map(|s| SpaceId::linked(&s)).collect())
    }

    /// Whether `scope` names a Space that is not there: only a desktop-wide Space can be gone
    /// (`desktop`, every Space, and an app's own Space are not in the registry).
    fn lacks(&self, scope: &SpaceScope) -> Option<SpaceId> {
        match scope {
            SpaceScope::Only(space) => match space.kind() {
                SpaceKind::Linked(_) if !self.0.contains(space) => Some(space.clone()),
                SpaceKind::Linked(_) | SpaceKind::Outside | SpaceKind::App { .. } => None,
            },
            SpaceScope::Any => None,
        }
    }
}

/// Grants split into those that stay and the count of those that ended, by reason.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reconciled {
    /// The grants that remain.
    pub kept: Vec<ActionGrant>,
    /// What ended and why.
    pub ended: Vec<Ended>,
}

impl Reconciled {
    fn end(&mut self, why: GrantEnd) {
        match self.ended.iter_mut().find(|e| e.why == why) {
            Some(known) => known.count += 1,
            None => self.ended.push(Ended { why, count: 1 }),
        }
    }

    /// Whether anything ended.
    pub fn changed(&self) -> bool {
        !self.ended.is_empty()
    }
}

/// Why a grant ends, when it does.
fn fate(key: &ActionGrantKey, known: Option<&KnownSpaces>) -> Option<GrantEnd> {
    if let SpaceAccess::Refused(SpaceRefusal { space, .. }) = key.space_access() {
        return Some(GrantEnd::NotTheirSpace(space));
    }
    known
        .and_then(|k| k.lacks(&key.space))
        .map(GrantEnd::SpaceGone)
}

/// `grants` less those over another app's Space, and (when the registry is known) those over a
/// desktop-wide Space it does not hold.
pub fn reconcile(grants: Vec<ActionGrant>, known: Option<&KnownSpaces>) -> Reconciled {
    grants
        .into_iter()
        .fold(Reconciled::default(), |mut out, grant| {
            match fate(&grant.key, known) {
                Some(why) => out.end(why),
                None => out.kept.push(grant),
            }
            out
        })
}

/// `grants` less those scoped to exactly `gone`. A grant for every Space stays.
pub fn without_space(grants: Vec<ActionGrant>, gone: &SpaceId) -> Reconciled {
    grants
        .into_iter()
        .fold(Reconciled::default(), |mut out, grant| {
            match &grant.key.space {
                SpaceScope::Only(space) if space == gone => {
                    out.end(GrantEnd::SpaceGone(gone.clone()))
                }
                SpaceScope::Only(_) | SpaceScope::Any => out.kept.push(grant),
            }
            out
        })
}

/// Why a grant file is not a list of grants.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct GrantListFault(pub String);

/// The grants a file's text holds. An entry whose Space is not an id porter reads is dropped
/// and counted (never read as "every Space"); an entry that is wrong in any other way makes
/// the whole file a fault, so a damaged file is left alone and never rewritten.
pub fn decode_grants(text: &str) -> Result<Reconciled, GrantListFault> {
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(text).map_err(|why| GrantListFault(why.to_string()))?;
    entries
        .into_iter()
        .try_fold(Reconciled::default(), |mut out, entry| {
            match serde_json::from_value::<ActionGrant>(entry.clone()) {
                Ok(grant) => out.kept.push(grant),
                Err(_) if only_the_space_is_wrong(entry) => out.end(GrantEnd::SpaceUnreadable),
                Err(why) => return Err(GrantListFault(why.to_string())),
            }
            Ok(out)
        })
}

/// Whether the entry reads once its Space is set aside: then the Space was the fault.
fn only_the_space_is_wrong(mut entry: serde_json::Value) -> bool {
    let Some(space) = entry.pointer_mut("/key/space") else {
        return false;
    };
    *space = serde_json::json!({"kind": "any"});
    serde_json::from_value::<ActionGrant>(entry).is_ok()
}
