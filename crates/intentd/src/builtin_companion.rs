//! `org.quire.Companion`: `companion.task.start` opens a child session (its task policy never
//! wider than the parent's) and records `task.started`; `companion.task.message` sends a
//! message through the router's delivery path. Both are the router's own business
//! (`Router::companion_perform`); the provider reaches the router through a port that is
//! attached once the router exists, so the router that owns the provider is not owned by it.

use docket_client::IntentProvider;
use docket_core::{
    AppRefusal, EntityRef, FailText, Hit, Invocation, Outcome, Preview, SuggestAsk, UndoFault,
    UndoToken, ValidManifest,
};
use docket_router::{Router, Seams};
use prov::{Actor, EntityId};
use std::fmt;
use std::sync::{Arc, OnceLock, Weak};

type Perform = Box<dyn Fn(Invocation) -> Result<Outcome, AppRefusal> + Send + Sync>;

/// The way back from the provider to the router that hosts it. Empty until [`attach`]; a router
/// that has gone is "not ready" too.
///
/// [`attach`]: CompanionPort::attach
#[derive(Clone, Default)]
pub struct CompanionPort(Arc<OnceLock<Perform>>);

impl fmt::Debug for CompanionPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let attached = self.0.get().is_some();
        write!(f, "CompanionPort(attached: {attached})")
    }
}

impl CompanionPort {
    /// A port to no router yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Points the port at `router`, held weakly. Only the first attachment counts.
    pub fn attach<S: Seams + 'static>(&self, router: &Arc<Router<S>>) {
        let weak: Weak<Router<S>> = Arc::downgrade(router);
        let _ = self.0.set(Box::new(move |inv| match weak.upgrade() {
            Some(router) => router.companion_perform(inv),
            None => Err(not_ready()),
        }));
    }

    fn perform(&self, inv: Invocation) -> Result<Outcome, AppRefusal> {
        match self.0.get() {
            Some(perform) => perform(inv),
            None => Err(not_ready()),
        }
    }
}

fn not_ready() -> AppRefusal {
    AppRefusal::Failed(FailText("the router is not ready".to_owned()))
}

/// `org.quire.Companion`.
#[derive(Debug)]
pub struct CompanionProvider {
    manifest: ValidManifest,
    port: CompanionPort,
}

impl CompanionProvider {
    /// The provider for its shipped manifest, acting through `port`.
    pub fn new(manifest: ValidManifest, port: CompanionPort) -> Self {
        Self { manifest, port }
    }
}

impl IntentProvider for CompanionProvider {
    fn manifest(&self) -> &ValidManifest {
        &self.manifest
    }

    async fn perform(&self, inv: Invocation) -> Result<Outcome, AppRefusal> {
        self.port.perform(inv)
    }

    async fn dry_run(&self, inv: Invocation) -> Result<Preview, AppRefusal> {
        let _ = inv;
        Ok(Preview::None)
    }

    async fn undo(&self, token: UndoToken, actor: Actor) -> Result<(), UndoFault> {
        let _ = (token, actor);
        Err(UndoFault::Gone)
    }

    async fn search(&self, text: &str) -> Vec<Hit> {
        let _ = text;
        Vec::new()
    }

    async fn preview(&self, id: &EntityId) -> Preview {
        let _ = id;
        Preview::None
    }

    async fn suggest(&self, ask: SuggestAsk) -> Vec<EntityRef> {
        let _ = ask;
        Vec::new()
    }
}
