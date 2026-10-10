//! `org.quire.Checkpoints`: `checkpoints.restore` puts a session's workspace back to one of its
//! restore points. Saving the files as they are first, writing the point's files back and
//! logging the restore are the router's business (`Router::checkpoints_perform`, which holds the
//! store, the log and the clock); the provider reaches the router through a port that is attached
//! once the router exists, as `org.quire.Companion` does, so the router that owns the provider is
//! not owned by it.

use docket_client::IntentProvider;
use docket_core::{
    AppRefusal, EntityRef, FailText, Hit, Invocation, Outcome, Preview, SuggestAsk, UndoFault,
    UndoToken, ValidManifest,
};
use docket_router::{Router, Seams};
use prov::{Actor, EntityId};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock, Weak};

type Boxed<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// The two things the router answers for the provider.
struct Hooks {
    perform: Box<dyn Fn(Invocation) -> Boxed<Result<Outcome, AppRefusal>> + Send + Sync>,
    dry_run: Box<dyn Fn(Invocation) -> Boxed<Result<Preview, AppRefusal>> + Send + Sync>,
}

/// The way back from the provider to the router that hosts it. Empty until [`attach`]; a router
/// that has gone is "not ready" too.
///
/// [`attach`]: CheckpointsPort::attach
#[derive(Clone, Default)]
pub struct CheckpointsPort(Arc<OnceLock<Hooks>>);

impl fmt::Debug for CheckpointsPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CheckpointsPort(attached: {})", self.0.get().is_some())
    }
}

impl CheckpointsPort {
    /// A port to no router yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Points the port at `router`, held weakly. Only the first attachment counts.
    pub fn attach<S: Seams + 'static>(&self, router: &Arc<Router<S>>) {
        let for_perform: Weak<Router<S>> = Arc::downgrade(router);
        let for_dry_run = for_perform.clone();
        let _ = self.0.set(Hooks {
            perform: Box::new(move |inv| {
                let router = for_perform.clone();
                Box::pin(async move {
                    match router.upgrade() {
                        Some(router) => router.checkpoints_perform(inv).await,
                        None => Err(not_ready()),
                    }
                })
            }),
            dry_run: Box::new(move |inv| {
                let router = for_dry_run.clone();
                Box::pin(async move {
                    match router.upgrade() {
                        Some(router) => router.checkpoints_dry_run(inv).await,
                        None => Err(not_ready()),
                    }
                })
            }),
        });
    }

    async fn perform(&self, inv: Invocation) -> Result<Outcome, AppRefusal> {
        match self.0.get() {
            Some(hooks) => (hooks.perform)(inv).await,
            None => Err(not_ready()),
        }
    }

    async fn dry_run(&self, inv: Invocation) -> Result<Preview, AppRefusal> {
        match self.0.get() {
            Some(hooks) => (hooks.dry_run)(inv).await,
            None => Err(not_ready()),
        }
    }
}

fn not_ready() -> AppRefusal {
    AppRefusal::Failed(FailText("the router is not ready".to_owned()))
}

/// `org.quire.Checkpoints`.
#[derive(Debug)]
pub struct CheckpointsProvider {
    manifest: ValidManifest,
    port: CheckpointsPort,
}

impl CheckpointsProvider {
    /// The provider for its shipped manifest, acting through `port`.
    pub fn new(manifest: ValidManifest, port: CheckpointsPort) -> Self {
        Self { manifest, port }
    }
}

impl IntentProvider for CheckpointsProvider {
    fn manifest(&self) -> &ValidManifest {
        &self.manifest
    }

    async fn perform(&self, inv: Invocation) -> Result<Outcome, AppRefusal> {
        self.port.perform(inv).await
    }

    async fn dry_run(&self, inv: Invocation) -> Result<Preview, AppRefusal> {
        self.port.dry_run(inv).await
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
