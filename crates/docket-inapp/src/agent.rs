//! The in-app host: one app, one task at a time, the whole gate in process.

use crate::kit::{AuditTo, InAppKit};
use crate::link::ProviderLink;
use crate::seams::{AuditBuffer, InAppSeams, NoMemory, NoReader, NoWriter, SessionGrants};
use crate::sheet::{ConfirmSheet, SheetConfirmer};
use crate::turn::OpenTask;
use action_review::Reviewer;
use agent_loop::LoopInput;
use docket_client::{ClientError, ContextSource, InProcess, IntentProvider, Intents};
use docket_core::{
    AgentConfig, AuditRecord, CallRefusal, ContextKeep, Keep, NoteAsk, Origin, PolicyWriter,
    Reader, SessionOpen, StepLine, TurnIn, TurnSource, TurnVia, UserTurn,
};
use docket_memory::{AuditState, Report};
use docket_planner::{Catalogue, PlanFault, PlannerModel};
use docket_router::{Clock, GrantStore, MemoryLink, Registry, Router};
use policy_point::Pdp;
use porter_client::Transport as ModelTransport;
use porter_core::{AppId, AppName, Isolation};
use prov::{AgentRef, SpaceId};
use std::collections::BTreeSet;
use std::sync::Arc;

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
}

impl From<ClientError> for AgentFault {
    fn from(error: ClientError) -> Self {
        AgentFault::Router(error.to_string())
    }
}

/// Why a turn failed.
#[derive(Debug, Clone, PartialEq)]
pub enum Failure {
    /// The planner model gave nothing usable.
    Model(PlanFault),
    /// A call was refused in a way that ends the turn (over budget).
    Refused(CallRefusal),
    /// The quarantined reader could not answer.
    Reader,
    /// The turn spent its step budget.
    Budget,
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
    /// What the planner said to the person, in order.
    pub said: Vec<String>,
    /// The calls of this turn, oldest first.
    pub steps: Vec<StepLine>,
    /// How it ended.
    pub ending: Ending,
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

type Seam<P, C, T, R, K, G, Y, W, D> = InAppSeams<P, C, T, R, K, G, Y, W, D>;
type Link<P, C, T, R, K, G, Y, W, D> = Intents<InProcess<Seam<P, C, T, R, K, G, Y, W, D>>>;
type Hosted<P, C, T, R, K, G, Y, W, D> = Arc<Router<Seam<P, C, T, R, K, G, Y, W, D>>>;

/// One app's agent: `ask` runs a turn end to end through the router.
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
    /// The planner's side: role `companion`, whose voice the router never believes.
    pub(crate) intents: Link<P, C, T, R, K, G, Y, W, D>,
    /// The person's side: role `field`, the app's own prompt, which alone records turns.
    person: Link<P, C, T, R, K, G, Y, W, D>,
    pub(crate) planner: PlannerModel<M>,
    pub(crate) config: AgentConfig,
    pub(crate) app: AppName,
    pub(crate) space: SpaceId,
    pub(crate) clock: K,
    task: Option<OpenTask>,
    audit: AuditTo,
    audit_state: AuditState,
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

const fn keep_nothing() -> ContextKeep {
    ContextKeep {
        query: Keep::Dropped,
        results: Keep::Dropped,
        selection: Keep::Dropped,
        window: Keep::Dropped,
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
    /// one it acts in, and `field` would speak with the person's voice.
    pub fn with_kit(
        parts: InAppParts<P, C, T, R, M, K>,
        kit: InAppKit<G, Y, W, D>,
    ) -> Result<Self, AgentFault> {
        let manifest = parts.provider.manifest().clone();
        let app = manifest.manifest().app.clone();
        let seams = InAppSeams {
            link: ProviderLink::new(parts.provider, parts.context),
            confirmer: SheetConfirmer::new(parts.sheet, parts.clock.clone()),
            reviewer: parts.reviewer,
            grants: kit.grants,
            sink: AuditBuffer::default(),
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
        let caller = |role| docket_core::CallerId {
            app: AppId {
                name: app.clone(),
                isolation: Isolation::Unsandboxed,
            },
            roles: BTreeSet::from([role]),
        };
        Ok(Self {
            intents: Intents::over(InProcess::new(
                router.clone(),
                caller(docket_core::CallerRole::Companion),
            )),
            person: Intents::over(InProcess::new(
                router.clone(),
                caller(docket_core::CallerRole::Field),
            )),
            router,
            planner: PlannerModel::new(parts.model),
            config: parts.config,
            app,
            space: parts.space,
            clock: parts.clock,
            task: None,
            audit: kit.audit,
            audit_state: AuditState::default(),
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

    /// The router, for the app to read what its seams hold (the sheet, the consent store).
    pub fn router(&self) -> &Router<Seam<P, C, T, R, K, G, Y, W, D>> {
        &self.router
    }

    /// The person says `text`; the agent plans, calls the app's actions through the gate (the
    /// sheet asks where the gate says so) and runs until it is done, asks, or cannot go on.
    pub async fn ask(&mut self, text: &str) -> Result<Reply, AgentFault> {
        let manifests = self.intents.manifests().await?;
        self.planner
            .set_catalogue(Catalogue::from_manifests(&manifests));
        let mut task = match self.task.take() {
            Some(open) => open,
            None => self.open().await?,
        };
        let turn = self
            .person
            .session_turn(
                task.session.clone(),
                TurnIn {
                    text: text.to_owned(),
                    origin: Origin::InWindowField,
                    keep: keep_nothing(),
                    via: TurnVia::Typed,
                },
            )
            .await?;
        task.turns.push(UserTurn {
            id: turn,
            text: text.to_owned(),
            at: self.clock.now(),
            from: TurnSource::Field(self.app.clone()),
            via: TurnVia::Typed,
        });
        let before = task.history.len();
        let said = task.said.len();
        let (ending, mut task) = self.run(task, LoopInput::Asked(turn)).await;
        let reply = Reply {
            said: task.said.split_off(said),
            steps: task.history[before..].to_vec(),
            ending,
        };
        match reply.ending {
            Ending::Asked { .. } | Ending::Paused(_) => self.task = Some(task),
            _ => {
                // The router leaves the task's episode itself, as it does for companiond.
                let _ = self
                    .intents
                    .session_note(task.session.clone(), NoteAsk::End)
                    .await;
                let _ = self.intents.session_close(task.session).await;
            }
        }
        if self.audit == AuditTo::Memory {
            self.flush_audit().await;
        }
        Ok(reply)
    }

    /// Writes the audit records waiting in the buffer into memory (as almanac records in the
    /// agent's Space, the task's episode among them). What memory cannot take, because it is
    /// away or busy, stays queued for the next flush; the report says what happened. `ask` does
    /// this at the end of every turn under [`AuditTo::Memory`]; an app may also call it before
    /// it quits.
    pub async fn flush_audit(&mut self) -> Report {
        let space = self.space.clone();
        self.audit_state
            .flush(
                &self.router.seams.memory,
                self.router.seams.sink.queue(),
                move |_| Some(space.clone()),
            )
            .await
    }

    async fn open(&self) -> Result<OpenTask, AgentFault> {
        let opened = self
            .intents
            .session_open(SessionOpen {
                space: self.space.clone(),
                agent: AgentRef::Companion,
                parent: None,
            })
            .await?;
        Ok(OpenTask::new(opened.session))
    }
}
