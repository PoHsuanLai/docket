//! `MemoryStore`: a store over a folder held in memory, for tests and fakes. It keeps the same
//! contract as a real one (`contract`), including files the folder ignores, which no snapshot or
//! restore touches.

use crate::ids::{EntryId, Saved, TreeId, WorkRoot};
use crate::store::{
    ApplyAsk, CheckpointStore, DropAsk, PlanAsk, StoreFault, TakeAsk,
};
use crate::tree::{TreeListing, plan_restore};
use docket_core::{CheckpointId, RestorePlan, WorkPath};
use porter_core::Count;
use prov::SessionId;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Whether the folder's ignore rules cover a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tracking {
    /// Snapshots and restores cover it.
    Followed,
    /// The folder ignores it: never saved, never listed, never touched.
    Ignored,
}

#[derive(Debug, Clone)]
struct File {
    content: String,
    tracking: Tracking,
}

#[derive(Debug, Clone)]
struct Point {
    saved: Saved,
    files: BTreeMap<WorkPath, String>,
}

#[derive(Debug, Default)]
struct State {
    folders: BTreeMap<WorkRoot, BTreeMap<WorkPath, File>>,
    without_history: BTreeSet<WorkRoot>,
    points: BTreeMap<(WorkRoot, SessionId), BTreeMap<CheckpointId, Point>>,
}

/// An in-memory folder and the points saved from it.
#[derive(Debug, Default)]
pub struct MemoryStore {
    state: Mutex<State>,
}

impl MemoryStore {
    /// A store with no folders; any folder it is asked about is empty and has history.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Makes `root` a folder that cannot keep points (`NoHistory`).
    pub fn lack_history(&self, root: &WorkRoot) {
        self.lock().without_history.insert(root.clone());
    }

    /// Writes (or replaces) a file in `root`.
    pub fn write(&self, root: &WorkRoot, path: &WorkPath, content: &str, tracking: Tracking) {
        self.lock().folders.entry(root.clone()).or_default().insert(
            path.clone(),
            File {
                content: content.to_owned(),
                tracking,
            },
        );
    }

    /// Deletes a file in `root`.
    pub fn remove(&self, root: &WorkRoot, path: &WorkPath) {
        if let Some(folder) = self.lock().folders.get_mut(root) {
            folder.remove(path);
        }
    }

    /// A file's content in `root`, ignored files included.
    pub fn read(&self, root: &WorkRoot, path: &WorkPath) -> Option<String> {
        let state = self.lock();
        let file = state.folders.get(root)?.get(path)?;
        Some(file.content.clone())
    }

    fn history(state: &State, root: &WorkRoot) -> Result<(), StoreFault> {
        if state.without_history.contains(root) {
            Err(StoreFault::NoHistory)
        } else {
            Ok(())
        }
    }

    /// The followed files of `root` as they are now.
    fn followed(state: &State, root: &WorkRoot) -> BTreeMap<WorkPath, String> {
        state
            .folders
            .get(root)
            .into_iter()
            .flatten()
            .filter(|(_, file)| file.tracking == Tracking::Followed)
            .map(|(path, file)| (path.clone(), file.content.clone()))
            .collect()
    }

    fn listing(files: &BTreeMap<WorkPath, String>) -> TreeListing {
        TreeListing(
            files
                .iter()
                .map(|(path, content)| (path.clone(), EntryId::new(content.clone())))
                .collect(),
        )
    }

    fn plan_for(
        state: &State,
        root: &WorkRoot,
        session: &SessionId,
        id: CheckpointId,
    ) -> Result<(RestorePlan, BTreeMap<WorkPath, String>), StoreFault> {
        Self::history(state, root)?;
        let point = state
            .points
            .get(&(root.clone(), session.clone()))
            .and_then(|held| held.get(&id))
            .ok_or(StoreFault::NoSuchPoint)?;
        let now = Self::listing(&Self::followed(state, root));
        let plan = plan_restore(&now, &Self::listing(&point.files));
        Ok((plan, point.files.clone()))
    }
}

impl CheckpointStore for MemoryStore {
    async fn take(&self, ask: TakeAsk) -> Result<Saved, StoreFault> {
        let mut state = self.lock();
        Self::history(&state, &ask.root)?;
        let files = Self::followed(&state, &ask.root);
        if files.len() > usize::try_from(ask.max.0).unwrap_or(usize::MAX) {
            return Err(StoreFault::TooLarge);
        }
        let held = state
            .points
            .entry((ask.root.clone(), ask.session.clone()))
            .or_default();
        if held.contains_key(&ask.id) {
            return Err(StoreFault::Failed);
        }
        let saved = Saved {
            id: ask.id,
            at: ask.at,
            tree: TreeId::new(format!("tree-{}-{}", ask.session, ask.id.0)),
        };
        held.insert(
            ask.id,
            Point {
                saved: saved.clone(),
                files,
            },
        );
        Ok(saved)
    }

    async fn held(&self, root: &WorkRoot, session: &SessionId) -> Result<Vec<Saved>, StoreFault> {
        let state = self.lock();
        Self::history(&state, root)?;
        Ok(state
            .points
            .get(&(root.clone(), session.clone()))
            .into_iter()
            .flat_map(|held| held.values().map(|point| point.saved.clone()))
            .collect())
    }

    async fn plan(&self, ask: PlanAsk) -> Result<RestorePlan, StoreFault> {
        let state = self.lock();
        Self::plan_for(&state, &ask.root, &ask.session, ask.id).map(|(plan, _)| plan)
    }

    async fn apply(&self, ask: ApplyAsk) -> Result<RestorePlan, StoreFault> {
        let mut state = self.lock();
        let (plan, then) = Self::plan_for(&state, &ask.root, &ask.session, ask.id)?;
        if plan.digest != ask.expect {
            return Err(StoreFault::PlanStale);
        }
        let folder = state.folders.entry(ask.root.clone()).or_default();
        for path in plan.changed.iter().chain(&plan.added) {
            if let Some(content) = then.get(path) {
                folder.insert(
                    path.clone(),
                    File {
                        content: content.clone(),
                        tracking: Tracking::Followed,
                    },
                );
            }
        }
        for path in &plan.removed {
            folder.remove(path);
        }
        Ok(plan)
    }

    async fn drop_points(&self, ask: DropAsk) -> Result<Count, StoreFault> {
        let mut state = self.lock();
        Self::history(&state, &ask.root)?;
        let dropped = state
            .points
            .get_mut(&(ask.root.clone(), ask.session.clone()))
            .map_or(0, |held| {
                ask.ids
                    .iter()
                    .filter(|id| held.remove(*id).is_some())
                    .count()
            });
        Ok(Count(u32::try_from(dropped).unwrap_or(u32::MAX)))
    }
}
