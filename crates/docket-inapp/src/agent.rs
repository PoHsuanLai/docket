//! The in-app host: one app, one task at a time, the whole gate in process.

use crate::link::ProviderLink;
use crate::seams::{AuditBuffer, InAppSeams, NoMemory, NoReader, NoWriter, SessionGrants};
use crate::sheet::{ConfirmSheet, SheetConfirmer};
use crate::turn::OpenTask;
use action_review::Reviewer;
use agent_loop::{
    FinishedAs, LoopEffect, LoopInput, LoopPhase, ModelOutput, Sources, agent_step, assemble,
};
use companion_wire::{AnswerPhase, NeedsYou};
use docket_client::{ClientError, ContextSource, InProcess, IntentProvider, Intents};
use docket_core::{
    AgentConfig, AuditRecord, CallId, CallRefusal, CallRequest, ContextKeep, ContextView, HereView,
    Keep, Origin, ReadAsk, ReaderAsk, Reveal, SelectionView, SessionOpen, StepLine, TextTargetView,
    TurnIn, TurnSource, TurnVia, UserTurn, VisibleView, WireRefusal,
};
use docket_planner::{Catalogue, PlanFault, PlannerModel};
use docket_router::{Clock, Registry, Router};
use policy_point::Pdp;
use porter_client::Transport as ModelTransport;
use porter_core::{AppId, AppName, Count, Isolation};
use prov::{AgentRef, Effect, SpaceId};
use std::collections::{BTreeSet, VecDeque};
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

type Seam<P, C, T, R, K> = InAppSeams<P, C, T, R, K>;
type Link<P, C, T, R, K> = Intents<InProcess<Seam<P, C, T, R, K>>>;
type Hosted<P, C, T, R, K> = Arc<Router<Seam<P, C, T, R, K>>>;

/// One app's agent: `ask` runs a turn end to end through the router.
pub struct InAppAgent<P, C, T, R, M: ModelTransport, K>
where
    P: IntentProvider + 'static,
    C: ContextSource + 'static,
    T: ConfirmSheet + 'static,
    R: Reviewer + 'static,
    K: Clock + Clone + 'static,
{
    router: Hosted<P, C, T, R, K>,
    /// The planner's side: role `companion`, whose voice the router never believes.
    intents: Link<P, C, T, R, K>,
    /// The person's side: role `field`, the app's own prompt, which alone records turns.
    person: Link<P, C, T, R, K>,
    planner: PlannerModel<M>,
    config: AgentConfig,
    app: AppName,
    space: SpaceId,
    clock: K,
    task: Option<OpenTask>,
}

impl<P, C, T, R, M: ModelTransport, K> std::fmt::Debug for InAppAgent<P, C, T, R, M, K>
where
    P: IntentProvider + 'static,
    C: ContextSource + 'static,
    T: ConfirmSheet + 'static,
    R: Reviewer + 'static,
    K: Clock + Clone + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "InAppAgent({})", self.app)
    }
}

fn nowhere(app: AppName) -> ContextView {
    ContextView {
        app,
        window: Reveal::Plain(String::new()),
        here: HereView::Nowhere,
        selection: SelectionView::Nothing,
        visible: VisibleView {
            kind: None,
            items: Vec::new(),
            total: Count(0),
        },
        text_target: TextTargetView::None,
    }
}

fn refusal_of(error: ClientError) -> CallRefusal {
    match error {
        ClientError::Refused(WireRefusal::Call(refusal)) => refusal,
        ClientError::Refused(_) | ClientError::Transport(_) | ClientError::Unexpected => {
            CallRefusal::Timeout
        }
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
    /// Builds the router over the app's provider and installs its manifest. The host speaks to
    /// the router as two callers of the app's own name and different roles: `field` (the app's
    /// own prompt records the person's turns, so the task policy is capped to this app plus
    /// reads) and `companion` (the planner's calls, whose labels the router derives itself).
    /// They are never one caller with both roles: the first role that may make a call is the
    /// one it acts in, and `field` would speak with the person's voice.
    pub fn new(parts: InAppParts<P, C, T, R, M, K>) -> Result<Self, AgentFault> {
        let manifest = parts.provider.manifest().clone();
        let app = manifest.manifest().app.clone();
        let seams = InAppSeams {
            link: ProviderLink::new(parts.provider, parts.context),
            confirmer: SheetConfirmer::new(parts.sheet, parts.clock.clone()),
            reviewer: parts.reviewer,
            grants: SessionGrants::default(),
            sink: AuditBuffer::default(),
            clock: parts.clock.clone(),
            memory: NoMemory,
            writer: NoWriter,
            reader: NoReader,
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
    pub fn router(&self) -> &Router<Seam<P, C, T, R, K>> {
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
                let _ = self.intents.session_close(task.session).await;
            }
        }
        Ok(reply)
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

    async fn run(&self, mut task: OpenTask, first: LoopInput) -> (Ending, OpenTask) {
        let mut queue = VecDeque::from([first]);
        let mut question = None;
        let mut failure = None;
        while let Some(input) = queue.pop_front() {
            let (next, effects) = agent_step(task.state.clone(), input);
            task.state = next;
            let mut position = 0u64;
            for effect in effects {
                match effect {
                    LoopEffect::AskPlanner => {
                        queue.extend(self.plan_turn(&mut task, &mut failure).await);
                    }
                    LoopEffect::Call(call) => {
                        let id = CallId(position);
                        position += 1;
                        queue.push_back(self.call(&mut task, *call, id).await);
                    }
                    LoopEffect::Read(ask) => {
                        queue.push_back(self.read(&mut task, *ask, &mut failure).await);
                    }
                    LoopEffect::Publish(AnswerPhase::NeedsYou(NeedsYou::Question {
                        text,
                        choices,
                    })) => question = Some((text, choices)),
                    LoopEffect::Refused(refusal) => {
                        if matches!(refusal, CallRefusal::OverBudget(_)) {
                            failure = Some(Failure::Refused(refusal));
                        }
                    }
                    LoopEffect::Held(call, why) => {
                        let effect = self.effect_of(&call);
                        task.hold(&call, effect, why);
                    }
                    LoopEffect::Publish(_) | LoopEffect::Note(_) | LoopEffect::CloseTask => {}
                }
            }
        }
        let ending = match task.state.phase {
            LoopPhase::Finished(FinishedAs::Done) => Ending::Done,
            LoopPhase::Finished(FinishedAs::Failed) => {
                Ending::Failed(failure.unwrap_or(Failure::Model(PlanFault::Unavailable)))
            }
            LoopPhase::Finished(FinishedAs::Cancelled) => Ending::Cancelled,
            LoopPhase::Paused(_) => Ending::Paused(question.map(|(t, _)| t).unwrap_or_default()),
            LoopPhase::Idle
            | LoopPhase::Planning
            | LoopPhase::AwaitingCalls
            | LoopPhase::AwaitingReader => {
                let (text, choices) = question.unwrap_or_default();
                Ending::Asked { text, choices }
            }
        };
        (ending, task)
    }

    async fn plan_turn(
        &self,
        task: &mut OpenTask,
        failure: &mut Option<Failure>,
    ) -> Vec<LoopInput> {
        if task.state.steps >= self.config.budget.calls.0 {
            *failure = Some(Failure::Budget);
            return vec![LoopInput::ModelFailed];
        }
        self.refresh_handles(task).await;
        let sources = self.sources(task).await;
        let view = assemble(&self.config.assembler, &sources);
        match self.planner.converse(&view).await {
            Err(fault) => {
                *failure = Some(Failure::Model(fault));
                vec![LoopInput::ModelFailed]
            }
            Ok(reply) => {
                let said = reply.said.map(|words| {
                    task.said.push(words.clone());
                    LoopInput::Planned(ModelOutput::Say(words))
                });
                said.into_iter()
                    .chain([LoopInput::Planned(reply.then)])
                    .collect()
            }
        }
    }

    async fn refresh_handles(&self, task: &mut OpenTask) {
        if let Ok(cards) = self.intents.session_handles(task.session.clone()).await {
            task.handles = cards;
        }
    }

    async fn sources(&self, task: &OpenTask) -> Sources {
        let task_policy = self
            .intents
            .session_task_policy(task.session.clone())
            .await
            .unwrap_or_default();
        Sources {
            cards: self.planner.catalogue().cards(),
            profile: Vec::new(),
            primer: None,
            rollup: None,
            roster: docket_core::Roster::default(),
            episodes: Vec::new(),
            recalled: Vec::new(),
            context: nowhere(self.app.clone()),
            turns: task.turns.clone(),
            history: task.history.clone(),
            handles: task.handles.clone(),
            inbox: Vec::new(),
            taint: task.taint(),
            task_policy,
            skills: Vec::new(),
            skill_texts: Vec::new(),
        }
    }

    fn effect_of(&self, call: &CallRequest) -> Effect {
        self.planner
            .catalogue()
            .of(&call.action)
            .map_or(Effect::Read, |t| t.decl.effect)
    }

    /// One call, through the router: gated, reviewed, confirmed on the app's sheet where the
    /// gate says so, performed by the app's own provider.
    async fn call(&self, task: &mut OpenTask, call: CallRequest, id: CallId) -> LoopInput {
        let effect = self.effect_of(&call);
        let result = self
            .intents
            .perform(call.clone(), Some(task.session.clone()), None)
            .await
            .unwrap_or_else(|error| Err(refusal_of(error)));
        LoopInput::CallEnded(id, task.record(call.action, effect, result))
    }

    async fn read(
        &self,
        task: &mut OpenTask,
        ask: ReaderAsk,
        failure: &mut Option<Failure>,
    ) -> LoopInput {
        match self
            .intents
            .session_read(task.session.clone(), ReadAsk { ask })
            .await
        {
            Ok(reveal) => LoopInput::ReadAnswered(reveal),
            Err(_) => {
                *failure = Some(Failure::Reader);
                LoopInput::ModelFailed
            }
        }
    }
}
