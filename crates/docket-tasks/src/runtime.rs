//! The runtime: the entry points the bus calls and the state they share. The companion is one
//! identity over many tasks; each task is its own docket session, run by the pure loop of
//! `agent-loop` whose effects `drive` carries out.

use crate::fault::ServeFault;
use crate::seams::{Now, Surface};
use crate::shared::Shared;
use crate::task::{TaskRuntime, roster_state};
use agent_loop::{FrontEvent, IdleState, LoopInput, LoopPhase, LoopState, SideTable, front_step};
use almanac_core::{Episode, EpisodeId};
use companion_wire::{AnswerPhase, AskWire, FrontTask, SessionRecord};
use docket_client::{Intents, Transport as IntentsTransport};
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
#[derive(Debug)]
pub struct Companion<P: InferTransport, I: IntentsTransport, K, S> {
    /// The router, as role `companion`.
    pub intents: Intents<I>,
    /// The planner.
    pub planner: PlannerModel<P>,
    /// The proposed values.
    pub config: AgentConfig,
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
}

fn idle_state() -> LoopState {
    LoopState {
        phase: LoopPhase::Idle,
        turn: None,
        steps: 0,
        pending: vec![],
        guard: Default::default(),
    }
}

impl<P: InferTransport, I: IntentsTransport, K: Now, S: Surface> Companion<P, I, K, S> {
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
            told: BTreeMap::new(),
            triggers: VecDeque::new(),
            unplaced: VecDeque::new(),
            running: BTreeSet::new(),
            booted: now,
            skill_files: Vec::new(),
            skills: docket_skills::Library::default(),
            manifests: Vec::new(),
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
                rt.inbox.extend(self.unplaced.drain(..));
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
            let _ = self.intents.session_close(session).await;
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
        rt.window = Some(ask.parent_window);
        rt.phase = AnswerPhase::Thinking;
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
        Roster { entries }
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
