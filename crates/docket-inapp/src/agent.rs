//! The in-app host: one app, many tasks, the whole gate in process. The tasks are
//! `docket-tasks`' (the same model companiond runs on the desktop): the host gives them the app's
//! own router, model and clock, and a quiet surface.

use crate::audit_file::{AuditFile, AuditFileError};
use crate::kit::{AuditTo, InAppKit};
use crate::link::ProviderLink;
use crate::seams::{AuditBuffer, InAppSeams, NoMemory, NoReader, NoWriter, SessionGrants};
use crate::sheet::{ConfirmSheet, SheetConfirmer};
use action_review::Reviewer;
use docket_client::{ClientError, ContextSource, InProcess, IntentProvider, Intents};
use docket_core::{AgentConfig, AuditRecord, CallRefusal, PolicyWriter, Reader, StepLine, TurnId};
use docket_memory::AuditState;
use docket_planner::{PlanFault, PlannerModel};
use docket_router::{Clock, GrantStore, MemoryLink, Registry, Router};
use docket_tasks::{Companion, Now, Quiet, ServeFault};
use policy_point::Pdp;
use porter_client::Transport as ModelTransport;
use porter_core::{AppId, AppName, Isolation};
use prov::{SpaceId, TaskId, UnixSeconds};
use std::collections::BTreeSet;
use std::sync::Arc;

pub use docket_tasks::Failure;

/// Why the host could not run a turn at all (a turn that ran and went badly is a [`Reply`]).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AgentFault {
    /// The shipped policy set did not load.
    #[error("policy: {0}")]
    Policy(policy_point::PolicyError),
    /// The app's manifest is not one the router accepts.
    #[error("manifest")]
    Manifest,
    /// The router refused a request of the host's own (a session, a turn).
    #[error("router: {0}")]
    Router(String),
    /// The task model could not do what was asked (no such session, a finished task).
    #[error("task: {0}")]
    Task(ServeFault),
    /// No task of this agent has that id.
    #[error("no such task")]
    NoSuchTask,
    /// The file of waiting audit records could not be read.
    #[error("audit queue: {0}")]
    AuditQueue(AuditFileError),
}

impl From<ClientError> for AgentFault {
    fn from(error: ClientError) -> Self {
        AgentFault::Router(error.to_string())
    }
}

impl From<ServeFault> for AgentFault {
    fn from(fault: ServeFault) -> Self {
        AgentFault::Task(fault)
    }
}

/// How a turn ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Ending {
    /// The planner finished.
    Done,
    /// The planner asks the person something; their answer is the next `ask`.
    Asked {
        /// The question.
        text: String,
        /// The choices offered.
        choices: Vec<String>,
    },
    /// The breaker paused the task: the person's next words resume it.
    Paused(String),
    /// The turn failed.
    Failed(Failure),
    /// A halt or cancel ended it.
    Cancelled,
}

/// What a turn did: the planner's words, every call and how it ended, and how the turn ended.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    /// The task the turn ran in.
    pub task: TaskId,
    /// What the planner said to the person, in order.
    pub said: Vec<String>,
    /// The calls of this turn, oldest first.
    pub steps: Vec<StepLine>,
    /// How it ended.
    pub ending: Ending,
}

impl From<PlanFault> for Ending {
    fn from(fault: PlanFault) -> Self {
        Ending::Failed(Failure::Model(fault))
    }
}

impl From<CallRefusal> for Ending {
    fn from(refusal: CallRefusal) -> Self {
        Ending::Failed(Failure::Refused(refusal))
    }
}

/// Everything the host is built from.
#[derive(Debug)]
pub struct InAppParts<P, C, T, R, M, K> {
    /// The app's own provider (its manifest is the only app the agent can reach).
    pub provider: P,
    /// What the person is looking at.
    pub context: C,
    /// The app's confirm sheet.
    pub sheet: T,
    /// The reviewer cascade (it can only tighten what the policy point allows).
    pub reviewer: R,
    /// The model: any `porter_client::Transport`.
    pub model: M,
    /// The clock.
    pub clock: K,
    /// The Space the app works in.
    pub space: SpaceId,
    /// The settings in force.
    pub config: AgentConfig,
}

/// The router's clock as the task model's.
#[derive(Debug, Clone)]
pub struct HostClock<K>(pub K);

impl<K: Clock> Now for HostClock<K> {
    fn now(&self) -> UnixSeconds {
        self.0.now()
    }
}

type Seam<P, C, T, R, K, G, Y, W, D> = InAppSeams<P, C, T, R, K, G, Y, W, D>;
pub(crate) type Link<P, C, T, R, K, G, Y, W, D> =
    Intents<InProcess<Seam<P, C, T, R, K, G, Y, W, D>>>;
type Hosting<P, C, T, R, K, G, Y, W, D> = Router<Seam<P, C, T, R, K, G, Y, W, D>>;
type Hosted<P, C, T, R, K, G, Y, W, D> = Arc<Hosting<P, C, T, R, K, G, Y, W, D>>;
/// The task model over this host's router link.
pub(crate) type Tasks<P, C, T, R, M, K, G, Y, W, D> =
    Companion<M, InProcess<Seam<P, C, T, R, K, G, Y, W, D>>, HostClock<K>, Quiet>;

/// A link to `router` as the app's own name in one role. A caller never holds two roles: the
/// first role that may make a call is the one it acts in.
fn link_of<P, C, T, R, K, G, Y, W, D>(
    router: &Hosted<P, C, T, R, K, G, Y, W, D>,
    app: &AppName,
    role: docket_core::CallerRole,
) -> Link<P, C, T, R, K, G, Y, W, D>
where
    P: IntentProvider + 'static,
    C: ContextSource + 'static,
    T: ConfirmSheet + 'static,
    R: Reviewer + 'static,
    K: Clock + Clone + 'static,
    G: GrantStore + 'static,
    Y: MemoryLink + 'static,
    W: PolicyWriter + 'static,
    D: Reader + 'static,
{
    let caller = docket_core::CallerId {
        app: AppId {
            name: app.clone(),
            isolation: Isolation::Unsandboxed,
        },
        roles: BTreeSet::from([role]),
    };
    Intents::over(InProcess::new(router.clone(), caller))
}

/// One app's agent: `ask` runs a turn end to end through the router, in the front task or a task
/// the app names.
pub struct InAppAgent<
    P,
    C,
    T,
    R,
    M: ModelTransport,
    K,
    G = SessionGrants,
    Y = NoMemory,
    W = NoWriter,
    D = NoReader,
> where
    P: IntentProvider + 'static,
    C: ContextSource + 'static,
    T: ConfirmSheet + 'static,
    R: Reviewer + 'static,
    K: Clock + Clone + 'static,
    G: GrantStore + 'static,
    Y: MemoryLink + 'static,
    W: PolicyWriter + 'static,
    D: Reader + 'static,
{
    pub(crate) router: Hosted<P, C, T, R, K, G, Y, W, D>,
    /// The person's side: role `field`, the app's own prompt, which alone records turns.
    pub(crate) person: Link<P, C, T, R, K, G, Y, W, D>,
    /// The tasks: the planner's side is role `companion`, whose voice the router never believes.
    pub(crate) tasks: Tasks<P, C, T, R, M, K, G, Y, W, D>,
    pub(crate) app: AppName,
    pub(crate) space: SpaceId,
    pub(crate) clock: K,
    pub(crate) audit: AuditTo,
    pub(crate) audit_state: AuditState,
    pub(crate) audit_file: Option<AuditFile>,
    pub(crate) audit_fault: Option<AuditFileError>,
    pub(crate) side_turns: u64,
}

impl<P, C, T, R, M: ModelTransport, K, G, Y, W, D> std::fmt::Debug
    for InAppAgent<P, C, T, R, M, K, G, Y, W, D>
where
    P: IntentProvider + 'static,
    C: ContextSource + 'static,
    T: ConfirmSheet + 'static,
    R: Reviewer + 'static,
    K: Clock + Clone + 'static,
    G: GrantStore + 'static,
    Y: MemoryLink + 'static,
    W: PolicyWriter + 'static,
    D: Reader + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "InAppAgent({})", self.app)
    }
}

impl<P, C, T, R, M: ModelTransport, K> InAppAgent<P, C, T, R, M, K>
where
    P: IntentProvider + 'static,
    C: ContextSource + 'static,
    T: ConfirmSheet + 'static,
    R: Reviewer + 'static,
    K: Clock + Clone + 'static,
{
    /// The agent with the stub seams: consent in memory, no memory, no policy writer, no reader.
    /// [`InAppAgent::with_kit`] takes the real parts an app chooses.
    pub fn new(parts: InAppParts<P, C, T, R, M, K>) -> Result<Self, AgentFault> {
        Self::with_kit(parts, InAppKit::default())
    }
}

impl<P, C, T, R, M: ModelTransport, K, G, Y, W, D> InAppAgent<P, C, T, R, M, K, G, Y, W, D>
where
    P: IntentProvider + 'static,
    C: ContextSource + 'static,
    T: ConfirmSheet + 'static,
    R: Reviewer + 'static,
    K: Clock + Clone + 'static,
    G: GrantStore + 'static,
    Y: MemoryLink + 'static,
    W: PolicyWriter + 'static,
    D: Reader + 'static,
{
    /// Builds the router over the app's provider and installs its manifest. The host speaks to
    /// the router as two callers of the app's own name and different roles: `field` (the app's
    /// own prompt records the person's turns, so the task policy is capped to this app plus
    /// reads) and `companion` (the planner's calls, whose labels the router derives itself).
    /// They are never one caller with both roles: the first role that may make a call is the
    /// one it acts in, and `field` would speak with the person's voice. Audit records a file
    /// held over from an earlier run are queued again, to be written on the next flush.
    pub fn with_kit(
        parts: InAppParts<P, C, T, R, M, K>,
        kit: InAppKit<G, Y, W, D>,
    ) -> Result<Self, AgentFault> {
        let manifest = parts.provider.manifest().clone();
        let app = manifest.manifest().app.clone();
        let sink = AuditBuffer::default();
        let audit_file = kit.audit_file;
        if let Some(file) = &audit_file {
            sink.queue().restore(file.waiting().to_vec());
        }
        let seams = InAppSeams {
            link: ProviderLink::new(parts.provider, parts.context),
            confirmer: SheetConfirmer::new(parts.sheet, parts.clock.clone()),
            reviewer: parts.reviewer,
            grants: kit.grants,
            sink,
            clock: parts.clock.clone(),
            memory: kit.memory,
            writer: kit.writer,
            reader: kit.reader,
        };
        let pdp = Pdp::standard().map_err(AgentFault::Policy)?;
        let router = Router::new(seams, parts.config, pdp);
        let mut registry = Registry::new();
        registry.insert(manifest);
        match router.state.lock() {
            Ok(mut state) => state.registry = registry,
            Err(poisoned) => poisoned.into_inner().registry = registry,
        }
        let router = Arc::new(router);
        let companion = link_of(&router, &app, docket_core::CallerRole::Companion);
        let tasks = Companion::new(
            companion,
            PlannerModel::new(parts.model),
            parts.config,
            HostClock(parts.clock.clone()),
            app.clone(),
        );
        Ok(Self {
            person: link_of(&router, &app, docket_core::CallerRole::Field),
            router,
            tasks,
            app,
            space: parts.space,
            clock: parts.clock,
            audit: kit.audit,
            audit_state: AuditState::default(),
            audit_file,
            audit_fault: None,
            side_turns: 0,
        })
    }

    /// The audit records so far (what the router decided and why, never content).
    pub fn audit(&self) -> Vec<AuditRecord> {
        self.router.seams.sink.records()
    }

    /// The app's provider, for the app to read what it holds.
    pub fn provider(&self) -> &P {
        self.router.seams.link.provider()
    }

    /// A link to this agent's router in the planner's role (`companion`) alone, for running a
    /// `docket-kit` agent in process with no bus and no daemon:
    /// `Agent::builder(model, agent.companion_link())`, then `ask_recorded` with a turn from
    /// [`InAppAgent::record_turn`].
    ///
    /// It is the very link the host's own tasks use, so it adds no authority: the router derives
    /// the labels of its calls itself, never believes its voice, and gates, confirms and audits
    /// every call as for the tasks. It cannot record a person's turn (`field` alone may), and it
    /// names the app's own name, so it reaches the app's own manifest only.
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # use docket_client::ContextSource;
    /// # use docket_core::{AgentConfig, ConfirmRequest, ContextScope, ContextSnapshot, ConfirmId};
    /// # use docket_core::{Here, Selection, TextTarget, Visible, WindowPrivacy};
    /// # use docket_fake::{FakeFiles, FixedClock, ScriptedReviewer, WordsModel, files_manifest};
    /// # use docket_inapp::{ConfirmSheet, InAppAgent, InAppParts, SheetAnswer};
    /// use docket_kit::{Actions, Agent, Asker, Catalogue, Ended};
    /// # use porter_core::{AppName, Count};
    /// # use prov::{Label, Labelled, SpaceId, UnixSeconds};
    /// # struct Nobody;
    /// # impl ConfirmSheet for Nobody {
    /// #     async fn ask(&self, _: &ConfirmRequest) -> SheetAnswer { SheetAnswer::Dismissed }
    /// # }
    /// # struct Nowhere(AppName);
    /// # impl ContextSource for Nowhere {
    /// #     fn snapshot(&self, _: ContextScope) -> ContextSnapshot {
    /// #         ContextSnapshot {
    /// #             app: self.0.clone(),
    /// #             window: Labelled { value: String::new(), label: Label::trusted_user() },
    /// #             here: Here::Nowhere,
    /// #             selection: Selection::Nothing,
    /// #             visible: Visible { kind: None, items: vec![], total: Count(0) },
    /// #             text_target: TextTarget::None,
    /// #             privacy: WindowPrivacy::Normal,
    /// #         }
    /// #     }
    /// # }
    /// # let space = SpaceId::parse("work")?;
    /// # let manifest = files_manifest()?;
    /// # let app = manifest.manifest().app.clone();
    /// // The app hosts its agent over its own provider (the neutral fake Files app here).
    /// # let parts = InAppParts {
    /// #     provider: FakeFiles::new(manifest, space.clone()),
    /// #     context: Nowhere(app.clone()),
    /// #     sheet: Nobody,
    /// #     reviewer: ScriptedReviewer::always_allow(),
    /// #     model: WordsModel::says("unused"),
    /// #     clock: FixedClock::at(UnixSeconds(1_000)),
    /// #     space: space.clone(),
    /// #     config: AgentConfig::default(),
    /// # };
    /// let host = InAppAgent::new(parts)?;
    ///
    /// // 1. The host records the person's words with its own person link.
    /// let recorded = host.record_turn("Anything to tidy?").await?;
    ///
    /// // 2. The kit agent runs over the companion-only link, on that record.
    /// let link = host.companion_link();
    /// let catalogue = Catalogue::from_manifests(&link.manifests().await?);
    /// let kit = Agent::builder(WordsModel::says("Nothing to change."), link)
    ///     .actions(Actions::from(&catalogue).app(app.as_str()).reads_only())
    ///     .build()?;
    /// let asker = Asker {
    ///     app: app.clone(),
    ///     space,
    ///     from: recorded.turn.from.clone(),
    ///     at: recorded.turn.at,
    /// };
    /// let run = kit
    ///     .ask_recorded(&asker, recorded.session.clone(), recorded.turn.clone())
    ///     .await;
    /// assert_eq!(run.ended, Ended::Done);
    /// assert_eq!(run.said, ["Nothing to change."]);
    ///
    /// // 3. The host closes the session it opened.
    /// host.end_recorded(recorded).await?;
    /// # Ok(()) }
    /// ```
    pub fn companion_link(&self) -> Link<P, C, T, R, K, G, Y, W, D> {
        link_of(&self.router, &self.app, docket_core::CallerRole::Companion)
    }

    /// The router, for the app to read what its seams hold (the sheet, the consent store).
    pub fn router(&self) -> &Hosting<P, C, T, R, K, G, Y, W, D> {
        &self.router
    }

    pub(crate) fn next_side_turn(&mut self) -> TurnId {
        self.side_turns += 1;
        TurnId(self.side_turns)
    }
}
