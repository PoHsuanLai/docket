//! Putting entries into the index from outside a request: the ones an app ships in its search
//! file, and the ones it pushed in an earlier login. Both keep only the kinds the manifest marks
//! as indexed, as a push does.

use crate::router::Router;
use crate::seams::Seams;
use crate::state::RouterState;
use docket_core::{IndexEntry, IndexPolicy, IndexState};
use porter_core::AppName;
use prov::{EntityKey, EntityKind};
use std::collections::BTreeMap;

/// The entries of `app` of a kind its manifest indexes, by kind and key; none when the app has
/// no manifest.
fn indexed_only(
    st: &RouterState,
    app: &AppName,
    entries: Vec<IndexEntry>,
) -> BTreeMap<(EntityKind, EntityKey), IndexEntry> {
    let Some(manifest) = st.registry.get(app) else {
        return BTreeMap::new();
    };
    let indexed = |kind: &EntityKind| {
        manifest
            .manifest()
            .entities
            .iter()
            .any(|e| e.index == IndexPolicy::Indexed && &e.kind == kind)
    };
    entries
        .into_iter()
        .filter(|e| indexed(&e.kind))
        .map(|e| ((e.kind.clone(), e.key.clone()), e))
        .collect()
}

impl<S: Seams> Router<S> {
    /// Installs the entries `app` ships, replacing what it shipped before. A `Reset` never
    /// removes them.
    pub fn ship_index(&self, app: &AppName, entries: Vec<IndexEntry>) {
        let mut st = self.locked();
        let kept = indexed_only(&st, app, entries);
        st.shipped.insert(app.clone(), kept);
    }

    /// Brings back what `app` pushed in an earlier login. Its index state stays unknown, so its
    /// next `Reset` or `Push` proceeds as for any app that has just started.
    pub fn restore_index(&self, app: &AppName, entries: Vec<IndexEntry>) {
        let mut st = self.locked();
        let kept = indexed_only(&st, app, entries);
        st.shadow.insert(app.clone(), kept);
        st.index.insert(app.clone(), IndexState::Unknown);
    }

    /// Whether a manifest of `app` is installed.
    pub fn has_manifest(&self, app: &AppName) -> bool {
        self.locked().registry.get(app).is_some()
    }

    /// The entries `app` has pushed now.
    pub fn pushed_entries(&self, app: &AppName) -> Vec<IndexEntry> {
        self.locked()
            .shadow
            .get(app)
            .map(|all| all.values().cloned().collect())
            .unwrap_or_default()
    }
}
