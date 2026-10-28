//! The manifests of every installed app. Reading a manifest file is `docket_core::parse`.

use docket_core::{ActionDecl, ManifestError, ValidManifest};
use docket_vocab::ActionRef;
use porter_core::AppName;
use prov::EntityKind;
use std::collections::{BTreeMap, BTreeSet};

/// The installed manifests, by app.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Registry {
    manifests: BTreeMap<AppName, ValidManifest>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Installs or replaces an app's manifest.
    pub fn insert(&mut self, manifest: ValidManifest) {
        self.manifests
            .insert(manifest.manifest().app.clone(), manifest);
    }

    /// Removes an app (uninstalled).
    pub fn remove(&mut self, app: &AppName) -> Option<ValidManifest> {
        self.manifests.remove(app)
    }

    /// One app's manifest.
    pub fn get(&self, app: &AppName) -> Option<&ValidManifest> {
        self.manifests.get(app)
    }

    /// Every manifest, by app.
    pub fn all(&self) -> impl Iterator<Item = &ValidManifest> {
        self.manifests.values()
    }

    /// One action's declaration.
    pub fn action(&self, action: &ActionRef) -> Option<&ActionDecl> {
        self.manifests
            .get(&action.app)?
            .manifest()
            .actions
            .iter()
            .find(|a| a.name == action.name)
    }

    /// Every kind some installed app declares.
    pub fn kinds(&self) -> BTreeSet<&EntityKind> {
        self.manifests
            .values()
            .flat_map(|m| m.manifest().entities.iter().map(|e| &e.kind))
            .collect()
    }

    /// The actions that name a kind no installed app declares. Only the registry can tell: it
    /// sees every manifest, and an app may name another app's kind.
    pub fn unresolved(&self) -> Vec<ManifestError> {
        use docket_core::TargetKind;
        use docket_vocab::ParamType;
        let known = self.kinds();
        let mut faults = Vec::new();
        for manifest in self.manifests.values() {
            for a in &manifest.manifest().actions {
                let named = match &a.on {
                    TargetKind::One(k) | TargetKind::Many(k) => Some(k),
                    TargetKind::Nothing | TargetKind::Text | TargetKind::Files => None,
                }
                .into_iter()
                .chain(a.params.iter().filter_map(|p| match &p.ty {
                    ParamType::Entity(k) | ParamType::Entities(k) => Some(k),
                    _ => None,
                }));
                for kind in named {
                    if !known.contains(kind) {
                        faults.push(ManifestError::UnknownKind {
                            action: a.name.clone(),
                            kind: kind.clone(),
                        });
                    }
                }
            }
            // A relation names a kind as an action does; its derived action carries the fault.
            for e in &manifest.manifest().entities {
                for r in e.relations.iter().filter(|r| !known.contains(&r.to)) {
                    if let Some(action) = docket_core::related_name(&e.kind) {
                        faults.push(ManifestError::UnknownKind {
                            action,
                            kind: r.to.clone(),
                        });
                    }
                }
            }
        }
        faults
    }
}
