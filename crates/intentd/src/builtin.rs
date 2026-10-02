//! The providers intentd hosts itself: `org.quire.Memory` (the planner reaches memory only
//! through these router actions, so the router can label what it delivers) and
//! `org.quire.Companion` (starting a worker task, messaging an agent).

use almanac_client::Transport as MemoryTransport;
use docket_client::IntentProvider;
use docket_core::{
    AppRefusal, EntityRef, Hit, Invocation, Outcome, Preview, SuggestAsk, UndoFault, UndoToken,
    ValidManifest,
};
use docket_router::{RegistryError, parse};
use prov::{Actor, EntityId};
use std::marker::PhantomData;

const MEMORY: &str = include_str!("../../../manifests/org.quire.Memory.toml");
const COMPANION: &str = include_str!("../../../manifests/org.quire.Companion.toml");

/// The built-in manifests, validated: Memory first, then Companion.
pub fn builtin_manifests() -> Result<Vec<ValidManifest>, RegistryError> {
    [MEMORY, COMPANION].into_iter().map(parse).collect()
}

macro_rules! builtin_provider {
    ($(#[$doc:meta])* $name:ident, $what:literal) => {
        $(#[$doc])*
        #[derive(Debug)]
        pub struct $name<T> {
            manifest: ValidManifest,
            _transport: PhantomData<fn() -> T>,
        }

        impl<T> $name<T> {
            /// The provider for its shipped manifest.
            pub fn new(manifest: ValidManifest) -> Self {
                Self { manifest, _transport: PhantomData }
            }
        }

        impl<T: MemoryTransport> IntentProvider for $name<T> {
            fn manifest(&self) -> &ValidManifest {
                &self.manifest
            }

            async fn perform(&self, inv: Invocation) -> Result<Outcome, AppRefusal> {
                let _ = inv;
                todo!($what)
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
    };
}

builtin_provider!(
    /// `org.quire.Memory`, over memoryd. Reads go as `Caller::Router` for the session's Space;
    /// `memory.propose` of untrusted text always lands in the pending queue.
    MemoryProvider,
    "MemoryProvider::perform: memory.recall -> Search, memory.facts -> Facts, memory.propose -> Propose (Staged), memory.forget -> PlanForget then Forget; results labelled from the hits"
);
builtin_provider!(
    /// `org.quire.Companion`: `companion.task.start` opens a child session (its task policy
    /// never wider than the parent's) and records `task.started`; `companion.task.message` sends
    /// a message through the router's delivery path.
    CompanionProvider,
    "CompanionProvider::perform: companion.task.start -> TaskStart::from_args, open the child session, record AuditRecord::TaskStarted, answer the task id; companion.task.message -> MessageDraft through the delivery path"
);
