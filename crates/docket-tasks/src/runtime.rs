//! The runtime: the entry points the bus calls and the state they share. The companion is one
//! identity over many tasks; each task is its own docket session, run by the pure loop of
//! `agent-loop` whose effects `drive` carries out.

use crate::best_effort::best_effort;
use crate::desk::HelperDesk;
use crate::fault::ServeFault;
use crate::handoff::Handoff;
use crate::helpers::AsHelper;
use crate::seams::Surface;
use crate::shared::Shared;
use crate::task::{TaskRuntime, roster_state};
use agent_loop::{FrontEvent, IdleState, LoopInput, LoopPhase, LoopState, SideTable, front_step};
use almanac_core::{Episode, EpisodeId};
use companion_wire::{AnswerPhase, AskWire, FrontTask, SessionRecord};
use docket_client::{Intents, Transport as IntentsTransport};
use docket_core::Clock;
use docket_core::{
    AgentConfig, EpisodeLine, LeadText, Reveal, Roster, RosterDetail, RosterFull, RosterLine,
    SessionOpen, SessionOpened,
};
use docket_planner::Catalogue;
use docket_planner::PlannerModel;
use porter_client::Transport as InferTransport;
use porter_core::AppName;
use prov::{AgentRef, SessionId, SpaceId, TaskId, UnixSeconds};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

/// How many finished tasks the roster and the recent-episodes section remember.
pub(crate) const REMEMBERED: usize = 8;

/// An ask that has begun: the task, the input its loop starts with, and its answer's path.
#[derive(Debug, Clone)]
pub struct Begun {
    /// The task.
    pub task: TaskId,
    /// What its loop is first told.
    pub first: LoopInput,
    /// The object path of its answer.
    pub path: String,
    /// What the person said, for the record of the session.
    pub asked: SessionRecord,
    /// The session.
    pub session: SessionId,
}

/// A finished task waiting for its narrative.
#[derive(Debug, Clone)]
pub(crate) struct Narration {
    pub episode: Episode,
    pub session: SessionId,
}

/// The companion.
///
/// Over the router in this process, with the neutral fake apps and a model that only talks: it
/// opens a task, and the task is the one running on its session.
///
/// ```
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use docket_client::{InProcess, Intents};
/// use docket_core::{AgentConfig, CallerId, CallerRole, SessionOpen};
/// use docket_fake::{WordsModel, fake_router};
/// use docket_planner::PlannerModel;
/// use docket_fake::FixedClock;
/// use docket_tasks::{Companion, Quiet};
/// use porter_core::{AppId, AppName, Isolation};
/// use prov::{AgentRef, SpaceId, UnixSeconds};
/// use std::{collections::BTreeSet, sync::Arc};
///
/// let router = Arc::new(fake_router(AgentConfig::default())?);
/// let shell = AppName::parse("org.quire.Shell")?;
/// let caller = CallerId {
///     app: AppId {
///         name: shell.clone(),
///         isolation: Isolation::Unsandboxed,
///     },
///     roles: BTreeSet::from([CallerRole::Companion]),
/// };
/// let mut companion: Companion<_, _, FixedClock, Quiet> = Companion::new(
///     Intents::over(InProcess::new(router, caller)),
///     PlannerModel::new(WordsModel::says("Done.")),
///     AgentConfig::default(),
///     FixedClock::at(UnixSeconds(1_000)),
///     shell,
/// );
/// let opened = companion
///     .open(SessionOpen {
///         space: SpaceId::parse("work")?,
///         agent: AgentRef::Companion,
///         parent: None,
///         cwd: None,
///         started_from: None,
///         external: None,
///         helper: None,
///         acts_for: None,
///     })
///     .await?;
/// assert_eq!(companion.task_of(&opened.session), Some(opened.task));
/// # Ok(()) }
/// ```
#[derive(Debug)]
pub struct Companion<P: InferTransport, I: IntentsTransport, K, S> {
    /// The router, as role `companion`.
    pub intents: Intents<I>,
    /// The planner.
    pub planner: PlannerModel<P>,
    /// The proposed values.
    pub config: AgentConfig,
    /// How long each kind of peer may take before the loop gives up on it.
    pub waits: docket_store::Waits,
    /// The task the launcher returns to.
    pub front: Option<TaskId>,
    /// Each task's loop.
    pub tasks: BTreeMap<TaskId, LoopState>,
    /// The person's side conversations with subagents.
    pub side: SideTable,
    /// The background narrative pass.
    pub idle: IdleState,
    /// What tells the time.
    pub clock: K,
    /// What the bus reads without waiting for the loop.
    pub shared: Arc<Shared<S>>,
    /// What each task keeps while it runs.
    pub runtimes: BTreeMap<TaskId, TaskRuntime>,
    /// The shell itself: where the person is when they summoned the companion from the launcher.
    pub shell: AppName,
    pub(crate) episodes: Vec<EpisodeLine>,
    pub(crate) narration: BTreeMap<EpisodeId, Narration>,
    /// Finished tasks whose router session is still open, oldest finish first.
    pub(crate) lingering: crate::linger::Lingering,
    pub(crate) known: BTreeMap<AgentRef, RosterLine>,
    pub(crate) run_ends: crate::run_ends::RunEnds,
    pub(crate) told: BTreeMap<AgentRef, LeadText>,
    pub(crate) triggers: VecDeque<(TaskId, LoopInput)>,
    pub(crate) unplaced: VecDeque<docket_core::InboundLine>,
    pub(crate) running: BTreeSet<TaskId>,
    pub(crate) booted: UnixSeconds,
    /// The installed skills that passed the format, before they are checked against the
    /// manifests (companiond reads them once, from directories its `main` was given).
    pub(crate) skill_files: Vec<docket_skills::Skill>,
    /// The skills whose actions are all registered, and the manifests they were checked against.
    pub(crate) skills: docket_skills::Library,
    pub(crate) manifests: Vec<docket_core::ValidManifest>,
    /// Whether the companion writes its roster notes (`Session.Note`) for its sessions.
    pub(crate) notes: Notes,
    /// What gives a turn to the coding agent the person chose as the assistant's model; none
    /// where nothing hosts agents (a test, an app that hosts the companion in itself).
    pub(crate) hand: Option<Arc<dyn Handoff>>,
    /// The helpers the host keeps: where the planner's two helper actions go, and where the roster
    /// learns their names; none where nothing keeps helpers.
    pub(crate) desk: Option<Arc<dyn HelperDesk>>,
    /// The helper this companion runs a job for, when it is a helper's: the roster leaves it out
    /// of the helpers its planner is told of, and its planner is offered only what its reach
    /// covers.
    pub as_helper: Option<AsHelper>,
    /// Which sessions are a helper's conversation, when the host has any.
    pub(crate) speaking: Option<crate::voices::Speaking>,
}

/// Whether a companion writes the legacy session notes (`companion.session.*`) a restart used to
/// rebuild the roster from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Notes {
    /// companiond: its sessions are on the roster.
    Written,
    /// An editor's host: its sessions are the editor's, not the companion's, and must not appear
    /// on the roster after a restart.
    Silent,
}

/// The loop of a task nothing has been asked of.
pub fn idle_state() -> LoopState {
    LoopState {
        phase: LoopPhase::Idle,
        turn: None,
        steps: 0,
        pending: vec![],
        guard: Default::default(),
    }
}

impl<P: InferTransport, I: IntentsTransport, K: Clock, S: Surface> Companion<P, I, K, S> {
    /// A companion with no tasks yet, over the surface's default.
    pub fn new(
        intents: Intents<I>,
        planner: PlannerModel<P>,
        config: AgentConfig,
        clock: K,
        shell: AppName,
    ) -> Self {
        let now = clock.now();
        Self {
            intents,
            planner,
            config,
            waits: docket_store::Waits::default(),
            front: None,
            tasks: BTreeMap::new(),
            side: SideTable::default(),
            idle: IdleState::quiet_since(now),
            clock,
            shared: Arc::new(Shared::new(S::default())),
            runtimes: BTreeMap::new(),
            shell,
            episodes: Vec::new(),
            narration: BTreeMap::new(),
            lingering: crate::linger::Lingering::default(),
            known: BTreeMap::new(),
            run_ends: crate::run_ends::RunEnds::default(),
            told: BTreeMap::new(),
            triggers: VecDeque::new(),
            unplaced: VecDeque::new(),
            running: BTreeSet::new(),
            booted: now,
            skill_files: Vec::new(),
            skills: docket_skills::Library::default(),
            notes: Notes::Written,
            manifests: Vec::new(),
            hand: None,
            desk: None,
            as_helper: None,
            speaking: None,
        }
    }

    /// The same companion, with the helpers `desk` keeps.
    pub fn with_desk(self, desk: Arc<dyn HelperDesk>) -> Self {
        Self {
            desk: Some(desk),
            ..self
        }
    }

    /// The helpers the host keeps, if it keeps any.
    pub fn desk(&self) -> Option<Arc<dyn HelperDesk>> {
        self.desk.clone()
    }

    /// Sets the helpers this companion's planner may give jobs to and propose.
    pub fn set_desk(&mut self, desk: Arc<dyn HelperDesk>) {
        self.desk = Some(desk);
    }

    /// The helpers the planner is told of: all but the one whose job this companion runs.
    pub(crate) fn helper_lines(&self) -> Vec<docket_core::HelperRosterLine> {
        self.desk
            .as_ref()
            .map(|desk| desk.roster_lines(self.as_helper.as_ref().map(|h| &h.id)))
            .unwrap_or_default()
    }

    /// The same companion, handing a turn to the coding agent the person chose through `hand`.
    pub fn with_handoff(self, hand: Arc<dyn Handoff>) -> Self {
        Self {
            hand: Some(hand),
            ..self
        }
    }

    /// Sets what hands a turn to the coding agent the person chose.
    pub fn set_handoff(&mut self, hand: Arc<dyn Handoff>) {
        self.hand = Some(hand);
    }

    /// The same companion, waiting on its peers as `waits` says.
    pub fn with_waits(self, waits: docket_store::Waits) -> Self {
        Self { waits, ..self }
    }

    /// The same companion, writing no roster notes: the host of an editor's sessions.
    pub fn without_notes(self) -> Self {
        Self {
            notes: Notes::Silent,
            ..self
        }
    }

    /// The installed skills this companion may offer (those that pass the format; the manifests
    /// decide which are reachable). Skills only teach: nothing here grants anything.
    pub fn with_skills(self, skill_files: Vec<docket_skills::Skill>) -> Self {
        Self {
            skill_files,
            ..self
        }
    }

    /// Replaces the installed skills, checked at once against the manifests last seen. Skills
    /// only teach: nothing here grants anything.
    pub fn install_skills(&mut self, skill_files: Vec<docket_skills::Skill>) {
        self.skills = docket_skills::Library::check(skill_files.clone(), &self.manifests);
        self.skill_files = skill_files;
    }

    /// Refreshes what the planner may call from the installed manifests. A router that does not
    /// answer leaves the last catalogue in place.
    pub(crate) async fn refresh_catalogue(&mut self) {
        if let Ok(manifests) = self.intents.manifests().await {
            self.skills = docket_skills::Library::check(self.skill_files.clone(), &manifests);
            self.manifests = manifests.clone();
            self.planner
                .set_catalogue(Catalogue::from_manifests(&manifests));
        }
    }

    /// `Companion1.Open`: opens a session for the front conversation.
    pub async fn open(&mut self, open: SessionOpen) -> Result<SessionOpened, ServeFault> {
        self.refresh_catalogue().await;
        let opened = self
            .intents
            .session_open(open.clone())
            .await
            .map_err(ServeFault::Router)?;
        self.adopt(&opened, &open);
        self.record(
            &opened.session,
            &SessionRecord::Opened {
                task: opened.task.clone(),
                space: open.space.clone(),
                agent: open.agent.clone(),
                parent: open.parent.clone(),
            },
        )
        .await;
        if open.agent == AgentRef::Companion {
            // Messages that came while no task of the companion could take them wait for it.
            if let Some(rt) = self.runtimes.get_mut(&opened.task) {
                for line in self.unplaced.drain(..) {
                    rt.note_message(&line);
                    rt.inbox.push(line);
                }
            }
            self.front = front_step(self.front.take(), &FrontEvent::Asked(opened.task.clone()));
        }
        self.publish();
        Ok(opened)
    }

    /// Takes a session the router opened into the companion's tasks.
    pub(crate) fn adopt(&mut self, opened: &SessionOpened, open: &SessionOpen) {
        self.runtimes.insert(
            opened.task.clone(),
            TaskRuntime::new(
                opened.session.clone(),
                open.space.clone(),
                open.agent.clone(),
                open.parent.clone(),
                self.clock.now(),
            ),
        );
        self.tasks.insert(opened.task.clone(), idle_state());
    }

    /// `Companion1.Close`: ends a session. A task still working is cancelled first. A finished
    /// task's answer is dismissed: its router session, held open so handles could still be shown,
    /// closes now.
    pub async fn close(&mut self, session: SessionId) -> Result<(), ServeFault> {
        let task = self.task_of(&session).ok_or(ServeFault::UnknownSession)?;
        let live = self
            .tasks
            .get(&task)
            .is_some_and(|s| !matches!(s.phase, LoopPhase::Finished(_)));
        self.record(&session, &SessionRecord::Closed).await;
        if live {
            self.run(&task, LoopInput::Cancelled).await?;
        }
        self.dismiss(&task).await;
        Ok(())
    }

    /// Closes a finished (or just cancelled) task's router session and lets its answer go.
    pub(crate) async fn dismiss(&mut self, task: &TaskId) {
        self.lingering = crate::linger::dismissed(std::mem::take(&mut self.lingering), task);
        if let Some(session) = self.runtimes.get(task).map(|rt| rt.session.clone()) {
            // The router answers either way (a session it already closed is not an error here).
            let who = session.clone();
            let closed = self.intents.session_close(session).await;
            best_effort("session.close", &who, closed);
        }
        if let Some(rt) = self.runtimes.remove(task) {
            self.remember_ended(task, &rt);
        }
        self.tasks.remove(task);
        self.front = front_step(self.front.take(), &FrontEvent::Ended(task.clone()));
        self.shared.drop_answer(task);
        self.publish();
    }

    /// The task running on `session`.
    pub fn task_of(&self, session: &SessionId) -> Option<TaskId> {
        self.runtimes
            .iter()
            .find(|(_, rt)| &rt.session == session)
            .map(|(task, _)| task.clone())
    }

    /// `Companion1.Ask`: starts the loop for a turn the UI already recorded and answers the
    /// object path of its answer, once the task has run as far as it can alone.
    pub async fn ask(&mut self, ask: AskWire) -> Result<String, ServeFault> {
        let begun = self.begin_ask(ask)?;
        self.record(&begun.session, &begun.asked).await;
        self.run_interactive(&begun.task, begun.first).await?;
        Ok(begun.path)
    }

    /// The first half of `ask`: the turn is the task's and the answer exists, thinking. The bus
    /// answers the path here and runs the rest in the background.
    pub fn begin_ask(&mut self, ask: AskWire) -> Result<Begun, ServeFault> {
        let task = self
            .task_of(&ask.session)
            .ok_or(ServeFault::UnknownSession)?;
        let phase = self.tasks.get(&task).map(|s| s.phase);
        if matches!(phase, Some(LoopPhase::Finished(_))) {
            return Err(ServeFault::Finished);
        }
        let Some(rt) = self.runtimes.get_mut(&task) else {
            return Err(ServeFault::UnknownSession);
        };
        let turn = ask.turn;
        let asked = SessionRecord::Asked {
            turn: turn.clone(),
            to: rt.agent.clone(),
            task: task.clone(),
        };
        let session = rt.session.clone();
        rt.turns.push(turn.clone());
        rt.keep = ask.keep;
        rt.mode = turn.mode.unwrap_or(rt.mode);
        rt.window = Some(ask.parent_window);
        rt.phase = AnswerPhase::Thinking;
        rt.progress.clear();
        rt.plan = crate::plan::Plan::default();
        rt.acts = crate::plan::Plan::default();
        rt.proposal = None;
        rt.failure = None;
        let first = match phase {
            Some(LoopPhase::Paused(_)) => LoopInput::Resumed,
            _ => LoopInput::Asked(turn.id),
        };
        rt.summoned = ask.app;
        self.front = front_step(self.front.take(), &FrontEvent::Asked(task.clone()));
        self.publish_answer(&task);
        Ok(Begun {
            path: self.shared.surface().answer_path(&task),
            task,
            first,
            asked,
            session,
        })
    }

    /// Runs what `begin_ask` began.
    pub async fn run_begun(&mut self, begun: Begun) -> Result<(), ServeFault> {
        self.record(&begun.session, &begun.asked).await;
        self.run_interactive(&begun.task, begun.first).await
    }

    /// The roster as the active Space may see it (another Space shows presence only).
    pub fn roster(&self) -> Roster {
        self.roster_without(None).seen_from(&self.active_space())
    }

    /// Every agent but `leave`, as lines. A task's own planner is told who else is working, not
    /// about itself: its own line changes with every step and would break the cached prefix.
    pub(crate) fn roster_without(&self, leave: Option<&TaskId>) -> Roster {
        let mut entries: Vec<RosterLine> = self
            .runtimes
            .iter()
            .filter(|(task, _)| Some(*task) != leave)
            .map(|(task, rt)| self.line_of(task, rt))
            .collect();
        entries.extend(self.known.values().cloned());
        Roster {
            entries,
            helpers: self.helper_lines(),
        }
    }

    /// The Space the person is working in: the front task's.
    pub(crate) fn active_space(&self) -> SpaceId {
        self.front
            .as_ref()
            .and_then(|t| self.runtimes.get(t))
            .map_or_else(SpaceId::desktop, |rt| rt.space.clone())
    }

    fn line_of(&self, task: &TaskId, rt: &TaskRuntime) -> RosterLine {
        let waiting = rt.turns.is_empty() && rt.inbox.is_empty();
        let state = match self.tasks.get(task) {
            Some(s) if s.phase == LoopPhase::Idle && waiting => docket_core::RosterState::Starting,
            other => roster_state(other, &rt.phase),
        };
        let goal = rt
            .turns
            .first()
            .map(|t| t.text.clone())
            .or_else(|| {
                rt.inbox.first().and_then(|m| {
                    m.parts.iter().find_map(|p| match p {
                        docket_core::InboundPart::Text(Reveal::Plain(t)) => Some(t.clone()),
                        _ => None,
                    })
                })
            })
            .map(|g| LeadText::of(&g).as_str().to_owned())
            .unwrap_or_default();
        RosterLine {
            agent: rt.agent.clone(),
            space: rt.space.clone(),
            state,
            detail: RosterDetail::Full(Box::new(RosterFull {
                goal: Reveal::Plain(goal),
                last: rt.history.last().cloned(),
                told: self.told.get(&rt.agent).cloned(),
                used: None,
                why: None,
            })),
        }
    }

    /// Remembers a finished task's roster line when its row closes.
    fn remember_ended(&mut self, task: &TaskId, rt: &TaskRuntime) {
        let line = self.line_of(task, rt);
        self.known.insert(rt.agent.clone(), line);
        while self.known.len() > REMEMBERED {
            let Some(oldest) = self.known.keys().next().cloned() else {
                break;
            };
            self.known.remove(&oldest);
        }
    }

    /// `Companion1.Front`.
    pub fn front_task(&self) -> FrontTask {
        FrontTask {
            task: self.front.clone(),
            session: self
                .front
                .as_ref()
                .and_then(|t| self.runtimes.get(t))
                .map(|rt| rt.session.clone()),
        }
    }

    /// Writes what the bus reads: the roster, the front task, and the answers.
    pub(crate) fn publish(&self) {
        self.shared.set_roster(self.roster());
        self.shared.set_front(self.front_task());
    }

    /// Writes one task's answer where the bus reads it.
    pub(crate) fn publish_answer(&self, task: &TaskId) {
        if let Some(rt) = self.runtimes.get(task) {
            self.shared.set_session(task, rt.session.clone());
            self.shared.set_route(task, rt.route.clone());
            self.shared.set_answer(rt.answer(task));
        }
        self.publish();
    }
}
