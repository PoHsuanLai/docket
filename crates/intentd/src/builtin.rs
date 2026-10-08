//! The providers intentd hosts itself: `org.quire.Memory` (the planner reaches memory only
//! through these router actions, so the router can label what it delivers) and
//! `org.quire.Companion` (starting a worker task, messaging one). `HostedLink` is the router's
//! `AppLink` with the two answered in process and every other app over D-Bus, and
//! `builtin_manifests` are what `quire-do apps` lists for them.

use crate::builtin_companion::{CompanionPort, CompanionProvider};
use crate::builtin_memory::{MEMORY_APP, MemoryProvider};
use crate::link::DbusLink;
use almanac_client::Transport as MemoryTransport;
use docket_client::IntentProvider;
use docket_core::{
    ACP_AGENT_APP, AppRefusal, ContextScope, ContextSnapshot, EntityRef, Generation, Hit,
    Invocation, Latency, Outcome, Preview, SuggestAsk, UndoFault, UndoToken, ValidManifest,
};
use docket_router::{AppFault, AppLink, COMPANION_APP, LinkFault, RegistryError, parse};
use porter_core::AppName;
use prov::{Actor, EntityId};

const MEMORY: &str = include_str!("../../../manifests/org.quire.Memory.toml");
const COMPANION: &str = include_str!("../../../manifests/org.quire.Companion.toml");
const ACP_AGENT: &str = include_str!("../../../manifests/org.quire.AcpAgent.toml");

/// The built-in manifests, validated: Memory, Companion, then the external agents' pseudo-app.
pub fn builtin_manifests() -> Result<Vec<ValidManifest>, RegistryError> {
    [MEMORY, COMPANION, ACP_AGENT]
        .into_iter()
        .map(parse)
        .collect()
}

/// Whether `app` is one of the three names whose declarations are the built-in ones: no
/// installed file may declare them. Memory and Companion are answered in process; the external
/// agents' pseudo-app (`org.quire.AcpAgent`) is answered by the host that launched the agent, on
/// that bus name, like an installed app.
pub fn is_builtin(app: &AppName) -> bool {
    matches!(app.as_str(), MEMORY_APP | COMPANION_APP | ACP_AGENT_APP)
}

/// Which provider an app name is.
enum Host {
    Memory,
    Companion,
    Installed,
}

fn host_of(app: &AppName) -> Host {
    match app.as_str() {
        MEMORY_APP => Host::Memory,
        COMPANION_APP => Host::Companion,
        _ => Host::Installed,
    }
}

/// Apps over D-Bus, and the two built-ins in process.
#[derive(Debug)]
pub struct HostedLink<T: MemoryTransport> {
    apps: DbusLink,
    memory: MemoryProvider<T>,
    companion: CompanionProvider,
}

impl<T: MemoryTransport> HostedLink<T> {
    /// Hosts the built-ins beside `apps`: Memory over `transport`, Companion through `port`.
    pub fn new(apps: DbusLink, transport: T, port: CompanionPort) -> Result<Self, RegistryError> {
        let [memory, companion, _agents] = <[ValidManifest; 3]>::try_from(builtin_manifests()?)
            .map_err(|_| RegistryError::Toml("the built-in manifests are not three".to_owned()))?;
        Ok(Self {
            apps,
            memory: MemoryProvider::new(memory, transport),
            companion: CompanionProvider::new(companion, port),
        })
    }

    /// The manifests of what is hosted, for the registry.
    pub fn manifests(&self) -> [&ValidManifest; 2] {
        [self.memory.manifest(), self.companion.manifest()]
    }
}

fn faulted(refusal: AppRefusal) -> AppFault {
    AppFault::Refused(refusal)
}

impl<T: MemoryTransport> AppLink for HostedLink<T> {
    async fn perform(
        &self,
        app: &AppName,
        inv: Invocation,
        within: Latency,
    ) -> Result<Outcome, AppFault> {
        match host_of(app) {
            Host::Memory => self.memory.perform(inv).await.map_err(faulted),
            Host::Companion => self.companion.perform(inv).await.map_err(faulted),
            Host::Installed => self.apps.perform(app, inv, within).await,
        }
    }

    async fn perform_activated(
        &self,
        app: &AppName,
        inv: Invocation,
        activation: Option<docket_core::ActivationToken>,
        within: Latency,
    ) -> Result<Outcome, AppFault> {
        match host_of(app) {
            Host::Installed => {
                self.apps
                    .perform_activated(app, inv, activation, within)
                    .await
            }
            Host::Memory | Host::Companion => self.perform(app, inv, within).await,
        }
    }

    async fn perform_classified(
        &self,
        app: &AppName,
        inv: Invocation,
        activation: Option<docket_core::ActivationToken>,
        classified: Option<docket_core::CallClass>,
        within: Latency,
    ) -> Result<Outcome, AppFault> {
        match host_of(app) {
            Host::Installed => {
                self.apps
                    .perform_classified(app, inv, activation, classified, within)
                    .await
            }
            Host::Memory | Host::Companion => self.perform(app, inv, within).await,
        }
    }

    async fn classify(
        &self,
        app: &AppName,
        inv: Invocation,
    ) -> Result<docket_core::CallClass, docket_core::ClassifyFault> {
        match host_of(app) {
            Host::Installed => self.apps.classify(app, inv).await,
            // The built-in providers declare every effect outright.
            Host::Memory | Host::Companion => Err(docket_core::ClassifyFault::Unsupported),
        }
    }

    async fn dry_run(&self, app: &AppName, inv: Invocation) -> Result<Preview, AppRefusal> {
        match host_of(app) {
            Host::Memory => self.memory.dry_run(inv).await,
            Host::Companion => self.companion.dry_run(inv).await,
            Host::Installed => self.apps.dry_run(app, inv).await,
        }
    }

    async fn undo(&self, app: &AppName, token: &UndoToken, actor: &Actor) -> Result<(), UndoFault> {
        match host_of(app) {
            Host::Memory => self.memory.undo(token.clone(), actor.clone()).await,
            Host::Companion => self.companion.undo(token.clone(), actor.clone()).await,
            Host::Installed => self.apps.undo(app, token, actor).await,
        }
    }

    async fn context(
        &self,
        app: &AppName,
        scope: ContextScope,
    ) -> Result<ContextSnapshot, LinkFault> {
        match host_of(app) {
            // Neither has a window: there is nothing on screen to report.
            Host::Memory | Host::Companion => Err(LinkFault::Unavailable),
            Host::Installed => self.apps.context(app, scope).await,
        }
    }

    async fn search(
        &self,
        app: &AppName,
        text: &str,
        generation: Generation,
    ) -> Result<Vec<Hit>, LinkFault> {
        match host_of(app) {
            Host::Memory => Ok(self.memory.search(text).await),
            Host::Companion => Ok(self.companion.search(text).await),
            Host::Installed => self.apps.search(app, text, generation).await,
        }
    }

    async fn preview(&self, app: &AppName, id: &EntityId) -> Result<Preview, LinkFault> {
        match host_of(app) {
            Host::Memory => Ok(self.memory.preview(id).await),
            Host::Companion => Ok(self.companion.preview(id).await),
            Host::Installed => self.apps.preview(app, id).await,
        }
    }

    async fn suggest(&self, app: &AppName, ask: SuggestAsk) -> Result<Vec<EntityRef>, LinkFault> {
        match host_of(app) {
            Host::Memory => Ok(self.memory.suggest(ask).await),
            Host::Companion => Ok(self.companion.suggest(ask).await),
            Host::Installed => self.apps.suggest(app, ask).await,
        }
    }
}
