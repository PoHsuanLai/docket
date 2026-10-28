//! The providers intentd hosts itself: `org.quire.Memory` (the planner reaches memory only
//! through these router actions, so the router can label what it delivers) and
//! `org.quire.Companion` (starting a worker task, messaging one), and `org.quire.Checkpoints`.
//! `HostedLink` is the router's `AppLink` with those answered in process and every other app over
//! D-Bus, and `builtin_manifests` are what `quire-do apps` lists for them.

use crate::builtin_chain::{Builtins, Chain};
use crate::builtin_checkpoints::{CheckpointsPort, CheckpointsProvider};
use crate::builtin_companion::{CompanionPort, CompanionProvider};
use crate::link::DbusLink;
use almanac_client::Transport as MemoryTransport;
use docket_core::LinkFault;
use docket_core::{
    AppRefusal, ContextScope, ContextSnapshot, Invocation, Outcome, Preview, SuggestAsk, UndoFault,
};
use docket_manifest::{Latency, ValidManifest};
use docket_manifest::{ManifestFileError, parse};
use docket_memory::{MEMORY_APP, MemoryProvider};
use docket_permissions_core::ACP_AGENT_APP;
use docket_permissions_core::COMPANION_APP;
use docket_router::{AppFault, AppLink, CHECKPOINTS_APP};
use docket_vocab::{EntityRef, UndoToken};
use porter_core::AppName;
use prov::{Actor, EntityId};

const MEMORY: &str = include_str!("../../../manifests/org.quire.Memory.toml");
const COMPANION: &str = include_str!("../../../manifests/org.quire.Companion.toml");
const ACP_AGENT: &str = include_str!("../../../manifests/org.quire.AcpAgent.toml");
const CHECKPOINTS: &str = include_str!("../../../manifests/org.quire.Checkpoints.toml");

/// The built-in manifests, validated: Memory, Companion, the external agents' pseudo-app, then
/// Checkpoints.
pub fn builtin_manifests() -> Result<Vec<ValidManifest>, ManifestFileError> {
    [MEMORY, COMPANION, ACP_AGENT, CHECKPOINTS]
        .into_iter()
        .map(parse)
        .collect()
}

/// Whether `app` is one of the four names whose declarations are the built-in ones: no
/// installed file may declare them. Memory, Companion and Checkpoints are answered in process; the external
/// agents' pseudo-app (`org.quire.AcpAgent`) is answered by the host that launched the agent, on
/// that bus name, like an installed app.
pub fn is_builtin(app: &AppName) -> bool {
    matches!(
        app.as_str(),
        MEMORY_APP | COMPANION_APP | CHECKPOINTS_APP | ACP_AGENT_APP
    )
}

/// Apps over D-Bus, and the built-ins in process.
#[derive(Debug)]
pub struct HostedLink<T: MemoryTransport> {
    apps: DbusLink,
    builtins: Chain<MemoryProvider<T>, Chain<CompanionProvider, CheckpointsProvider>>,
}

impl<T: MemoryTransport> HostedLink<T> {
    /// Hosts the built-ins beside `apps`: Memory over `transport`, Companion through `port`,
    /// Checkpoints through `checkpoints`.
    pub fn new(
        apps: DbusLink,
        transport: T,
        port: CompanionPort,
        checkpoints: CheckpointsPort,
    ) -> Result<Self, ManifestFileError> {
        let [memory, companion, _agents, restore] =
            <[ValidManifest; 4]>::try_from(builtin_manifests()?).map_err(|_| {
                ManifestFileError::Toml("the built-in manifests are not four".to_owned())
            })?;
        Ok(Self {
            apps,
            builtins: Chain(
                MemoryProvider::new(memory, transport),
                Chain(
                    CompanionProvider::new(companion, port),
                    CheckpointsProvider::new(restore, checkpoints),
                ),
            ),
        })
    }

    /// The manifests of what is hosted, for the registry.
    pub fn manifests(&self) -> Vec<&ValidManifest> {
        let mut out = Vec::new();
        self.builtins.manifests(&mut out);
        out
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
        if self.builtins.serves(app) {
            self.builtins
                .hosted_perform(app, inv)
                .await
                .map_err(faulted)
        } else {
            self.apps.perform(app, inv, within).await
        }
    }

    async fn perform_activated(
        &self,
        app: &AppName,
        inv: Invocation,
        activation: Option<docket_core::ActivationToken>,
        within: Latency,
    ) -> Result<Outcome, AppFault> {
        if self.builtins.serves(app) {
            self.perform(app, inv, within).await
        } else {
            self.apps
                .perform_activated(app, inv, activation, within)
                .await
        }
    }

    async fn perform_classified(
        &self,
        app: &AppName,
        inv: Invocation,
        activation: Option<docket_core::ActivationToken>,
        classified: Option<docket_vocab::CallClass>,
        within: Latency,
    ) -> Result<Outcome, AppFault> {
        if self.builtins.serves(app) {
            self.perform(app, inv, within).await
        } else {
            self.apps
                .perform_classified(app, inv, activation, classified, within)
                .await
        }
    }

    async fn classify(
        &self,
        app: &AppName,
        inv: Invocation,
    ) -> Result<docket_vocab::CallClass, docket_vocab::ClassifyFault> {
        if self.builtins.serves(app) {
            // The built-in providers declare every effect outright.
            Err(docket_vocab::ClassifyFault::Unsupported)
        } else {
            self.apps.classify(app, inv).await
        }
    }

    async fn dry_run(&self, app: &AppName, inv: Invocation) -> Result<Preview, AppRefusal> {
        if self.builtins.serves(app) {
            self.builtins.hosted_dry_run(app, inv).await
        } else {
            self.apps.dry_run(app, inv).await
        }
    }

    async fn undo(&self, app: &AppName, token: &UndoToken, actor: &Actor) -> Result<(), UndoFault> {
        if self.builtins.serves(app) {
            self.builtins
                .hosted_undo(app, token.clone(), actor.clone())
                .await
        } else {
            self.apps.undo(app, token, actor).await
        }
    }

    async fn running(&self, app: &AppName) -> bool {
        // Hosted in intentd itself, so running whenever the router is.
        self.builtins.serves(app) || self.apps.running(app).await
    }

    async fn context(
        &self,
        app: &AppName,
        scope: ContextScope,
    ) -> Result<ContextSnapshot, LinkFault> {
        if self.builtins.serves(app) {
            // None has a window: there is nothing on screen to report.
            Err(LinkFault::Unavailable)
        } else {
            self.apps.context(app, scope).await
        }
    }

    async fn preview(&self, app: &AppName, id: &EntityId) -> Result<Preview, LinkFault> {
        if self.builtins.serves(app) {
            Ok(self.builtins.hosted_preview(app, id).await)
        } else {
            self.apps.preview(app, id).await
        }
    }

    async fn suggest(&self, app: &AppName, ask: SuggestAsk) -> Result<Vec<EntityRef>, LinkFault> {
        if self.builtins.serves(app) {
            Ok(self.builtins.hosted_suggest(app, ask).await)
        } else {
            self.apps.suggest(app, ask).await
        }
    }
}
