//! The hub: the one owner of the board, and the place its jobs run from.
//!
//! A job runs as a tokio task of its own on a `Companion` of its own (built by the factory the
//! daemon passes in), so several helpers work at once, none behind the front conversation's lock,
//! and a panic or an error in one job ends that job: a second task awaits the first and turns
//! what it finds, a result or a `JoinError`, into the board's event for that job. The daemon and
//! every other job go on.

use crate::Companiond;
use crate::Shared;
use almanac_core::{RecentEntry, RecentQuery};
use docket_client::Transport as IntentsTransport;
use docket_core::helper::{
    AgentApps, AgentOutcome, Board, BoardEffect, BoardEvent, CallVerdict, CardRecord, HelperCard,
    HelperName, HelperRecord, HelperSeat, HelperState, JobFault, JobId, JobStatus, Launch, Need,
    Notice, ResultHandle, SeatPlace, SignInWay, TaskRef, owned_sentence, step,
};
use docket_core::routine::JobEnd;
use docket_core::{SessionOpen, ValidManifest};
use docket_tasks::{
    Change, JobEnding, JobInput, JobReport, JobSpec, OpenedJob, RecentSource, RouterRecent, Surface,
};
use futures_util::lock::Mutex as AsyncMutex;
use porter_client::Transport as InferTransport;
use prov::{AgentProgram, AgentRef, EntityId, HelperId, SessionId, SpaceId, TaskId};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::{AbortHandle, JoinError};

/// The most notices kept for the shell to read.
const MOST_NOTICES: usize = 20;
/// How many finished jobs keep their session open, so their answers can be handed on.
const KEPT_ANSWERS: usize = 16;
/// How long a stopped job has to end by itself before its task is cut off.
const GRACE: Duration = Duration::from_secs(5);

/// One sentence the person is told about a helper, once: the shell remembers the number of the
/// last one it showed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Told {
    /// Its number: later sentences have larger numbers.
    pub n: u64,
    /// The helper it is about.
    pub helper: HelperId,
    /// The sentence, in plain words.
    pub text: String,
}

/// Whether the hub closes the session an answer was given in when the answer is let go. A native
/// job's session is the hub's to close; an agent app's session is its process's, kept open across
/// the helper's jobs, and ends with that process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hold {
    Close,
    Leave,
}

/// What a finished job's answer is, while the process lives: the session it was given in and its
/// words. It is never written down: a handle to it expires with the session.
#[derive(Debug, Clone)]
struct Answer {
    session: SessionId,
    words: String,
    hold: Hold,
}

/// A job that is running: how to stop it, and where it runs.
#[derive(Debug)]
pub(super) struct Running {
    helper: HelperId,
    abort: Option<AbortHandle>,
    task: Option<TaskId>,
    session: Option<SessionId>,
    pub(super) shared: Option<Arc<Shared>>,
}

#[derive(Debug, Default)]
pub(super) struct State {
    pub(super) board: Board,
    told: VecDeque<Told>,
    said: u64,
    pub(super) running: BTreeMap<JobId, Running>,
    answers: BTreeMap<JobId, Answer>,
    kept: VecDeque<JobId>,
    /// Why a job stopped, as it was told, for its line on the card (not across a restart).
    whys: BTreeMap<JobId, String>,
    /// For a helper whose agent app wants signing in: the ways the app listed, by its own names.
    pub(super) signins: BTreeMap<HelperId, Vec<SignInWay>>,
    /// The helpers whose agent app opened a session, so is signed in: known only from the app.
    pub(super) signed: BTreeSet<HelperId>,
    /// The one session each helper has for the person to talk with it in, kept until the helper
    /// is removed. Not across a restart: the next pass opens it again.
    conversations: BTreeMap<HelperId, OpenedJob>,
    /// The routines proposed that wait for the person. Not across a restart.
    pub(super) proposals: super::proposals::Proposals,
    /// The helpers whose agent app said, after its last job, that the person could pick its
    /// conversation up in the app's own terminal program.
    pub(super) can_hand: BTreeSet<HelperId>,
    /// The helpers whose conversation is in the app's own terminal program: the session at the
    /// router that is a watch session meanwhile. Not across a restart.
    pub(super) away: BTreeMap<HelperId, SessionId>,
}

/// What builds the companion a job runs on: its transports are the daemon's, its planner is
/// `docket_tasks::planner_for` the card.
type Make<P, I> = dyn Fn(&HelperCard) -> Companiond<P, I> + Send + Sync;

struct Inner<P: InferTransport, I: IntentsTransport> {
    state: Mutex<State>,
    /// Reads and writes the records through the router. Nothing else uses it, so a record is
    /// never written behind a person's turn.
    service: AsyncMutex<Companiond<P, I>>,
    make: Box<Make<P, I>>,
    /// The companion each helper's conversation runs its turns on, made when the person first
    /// speaks and kept, so a turn sees the ones before it.
    talks: Mutex<BTreeMap<HelperId, Talk<P, I>>>,
    /// The host of the agent apps that are helpers, when the daemon has one.
    agents: std::sync::OnceLock<Arc<dyn AgentApps>>,
    /// The front companion's shared view: where the shell is told something changed.
    shared: Arc<Shared>,
}

/// The companion one helper's conversation runs its turns on.
type Talk<P, I> = Arc<AsyncMutex<Companiond<P, I>>>;

/// The board and what carries it out. Cheap to clone: every clone is the same board.
pub struct Hub<P: InferTransport, I: IntentsTransport> {
    inner: Arc<Inner<P, I>>,
}

impl<P: InferTransport, I: IntentsTransport> Clone for Hub<P, I> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<P: InferTransport, I: IntentsTransport> fmt::Debug for Hub<P, I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hub").finish_non_exhaustive()
    }
}

/// The Space a record belongs to: its helper's. A removed card's helper is no longer on the board
/// after the event, so the board before it is asked too.
fn space_of(record: &HelperRecord, before: &Board, after: &Board) -> Option<SpaceId> {
    let card = |id: &HelperId| {
        after
            .card(id)
            .or_else(|| before.card(id))
            .map(|c| c.space.clone())
    };
    match record {
        HelperRecord::Card(CardRecord::Kept(card_kept)) => Some(card_kept.space.clone()),
        HelperRecord::Card(CardRecord::Removed(id)) => card(id),
        HelperRecord::Job(job) => card(&job.owner),
        HelperRecord::Post(post) => card(&post.from),
        HelperRecord::Routine(routine) => card(routine.helper()),
    }
}

/// `records` grouped by Space, in the order the Spaces first appear.
fn by_space(
    records: impl IntoIterator<Item = HelperRecord>,
    before: &Board,
    after: &Board,
) -> Vec<(SpaceId, Vec<HelperRecord>)> {
    let mut groups: Vec<(SpaceId, Vec<HelperRecord>)> = Vec::new();
    for record in records {
        let Some(space) = space_of(&record, before, after) else {
            continue;
        };
        match groups.iter_mut().find(|(held, _)| *held == space) {
            Some((_, held)) => held.push(record),
            None => groups.push((space, vec![record])),
        }
    }
    groups
}

/// A job's run, as the task that ran it ends: the session it opened, if it got that far, and what
/// the run came to.
struct Finished {
    opened: Option<OpenedJob>,
    report: JobReport,
    hold: Hold,
}

/// What a job's end means for the board and for the router.
struct Conclusion {
    event: BoardEvent,
    answer: Option<Answer>,
    close: Option<SessionId>,
    calls: Vec<CallVerdict>,
    owned: Vec<EntityId>,
    why: Option<String>,
    /// The ways to sign in, when the job ended because its agent app wants the person to.
    ways: Option<Vec<SignInWay>>,
}

fn failure(job: JobId, fault: JobFault, close: Option<SessionId>) -> Conclusion {
    Conclusion {
        event: BoardEvent::Failed { job, fault },
        answer: None,
        close,
        calls: Vec::new(),
        owned: Vec::new(),
        why: None,
        ways: None,
    }
}

/// How a job's end is told to the board. A run that ended in a words answer keeps its session open
/// for its answer to be handed on; every other end closes it.
fn conclude(job: JobId, done: Finished) -> Conclusion {
    let Finished {
        opened,
        report,
        hold,
    } = done;
    let JobReport {
        ending,
        calls,
        owned,
        why,
    } = report;
    let close = opened.as_ref().map(|o| o.session.clone());
    let base = match (ending, opened) {
        (JobEnding::Done { words }, Some(opened)) => Conclusion {
            event: BoardEvent::Finished {
                job,
                result: ResultHandle {
                    task: opened.task,
                    session: opened.session.clone(),
                },
            },
            answer: Some(Answer {
                session: opened.session,
                words,
                hold,
            }),
            close: None,
            calls: Vec::new(),
            owned: Vec::new(),
            why: None,
            ways: None,
        },
        // Nothing failed: the job waits for the person to sign in, and so does its helper.
        (JobEnding::NeedsSignIn(ways), _) => Conclusion {
            event: BoardEvent::NeedsSignIn(job),
            ways: Some(ways),
            ..failure(job, JobFault::Crashed, close)
        },
        (JobEnding::Done { .. }, None) => failure(job, JobFault::Crashed, None),
        (JobEnding::Failed(fault), _) => failure(job, fault, close),
        (JobEnding::Stopped, _) => Conclusion {
            event: BoardEvent::Stopped(job),
            ..failure(job, JobFault::Crashed, close)
        },
    };
    Conclusion {
        calls,
        owned,
        why,
        ..base
    }
}

impl<P, I> Hub<P, I>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    /// A hub with an empty board. `service` is a companion of its own that reads and writes the
    /// helpers' records; `shared` is the front companion's view, where changes are told; `make`
    /// builds the companion a helper's job runs on.
    pub fn new(
        service: Companiond<P, I>,
        shared: Arc<Shared>,
        make: impl Fn(&HelperCard) -> Companiond<P, I> + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State::default()),
                service: AsyncMutex::new(service),
                make: Box::new(make),
                talks: Mutex::new(BTreeMap::new()),
                agents: std::sync::OnceLock::new(),
                shared,
            }),
        }
    }

    pub(super) fn with_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        match self.inner.state.lock() {
            Ok(mut state) => f(&mut state),
            Err(poisoned) => f(&mut poisoned.into_inner()),
        }
    }

    fn with_talks<R>(
        &self,
        f: impl FnOnce(&mut BTreeMap<HelperId, Arc<AsyncMutex<Companiond<P, I>>>>) -> R,
    ) -> R {
        match self.inner.talks.lock() {
            Ok(mut talks) => f(&mut talks),
            Err(poisoned) => f(&mut poisoned.into_inner()),
        }
    }

    /// The helper whose conversation `session` is, with its card as it is now. None for any other
    /// session, the front conversation's and a job's included.
    pub(super) fn conversation_at(&self, session: &SessionId) -> Option<(HelperCard, OpenedJob)> {
        self.with_state(|state| {
            let (helper, talk) = state
                .conversations
                .iter()
                .find(|(_, talk)| &talk.session == session)?;
            Some((state.board.card(helper)?.clone(), talk.clone()))
        })
    }

    /// A companion for `card`'s own planner, on the planner and transports the daemon gives that
    /// helper, with this board as its desk: the helper is told of the others, may hand one of them
    /// a job, and may suggest a routine for its card (the router gates each). Without the desk its
    /// planner was offered none of that.
    fn helpers_companion(&self, card: &HelperCard) -> Companiond<P, I> {
        let mut companion = (self.inner.make)(card);
        companion.set_desk(Arc::new(self.clone()));
        companion
    }

    /// The companion that runs the turns of `card`'s conversation, made the first time, on the
    /// planner and transports the daemon gives that helper.
    pub(super) fn talker(&self, card: &HelperCard) -> Arc<AsyncMutex<Companiond<P, I>>> {
        self.with_talks(|talks| {
            talks
                .entry(card.id.clone())
                .or_insert_with(|| Arc::new(AsyncMutex::new(self.helpers_companion(card))))
                .clone()
        })
    }

    /// The manifests of the installed apps, as the router lists them; none when it does not answer.
    pub(super) async fn manifests(&self) -> Vec<ValidManifest> {
        self.inner
            .service
            .lock()
            .await
            .intents
            .manifests()
            .await
            .unwrap_or_default()
    }

    /// The board as it stands.
    pub fn board(&self) -> Board {
        self.with_state(|state| state.board.clone())
    }

    /// The sentences told since the one numbered `after`, oldest first.
    pub fn told_since(&self, after: u64) -> Vec<Told> {
        self.with_state(|state| state.told.iter().filter(|t| t.n > after).cloned().collect())
    }

    /// Brings the board back from the records the Spaces hold, writes it again so the newest of
    /// each record leads, and gives each helper the open jobs that waited for it. A job that was
    /// running when the process ended waits to be asked again.
    pub async fn restore(&self, spaces: &[SpaceId]) {
        let records = self
            .inner
            .service
            .lock()
            .await
            .read_helper_records(spaces)
            .await;
        let board = Board::restore(records);
        let again = board.clone();
        self.with_state(|state| state.board = board);
        let groups = by_space(again.records(), &again, &again);
        {
            let service = self.inner.service.lock().await;
            for (space, records) in &groups {
                service.write_helper_records(space, records).await;
            }
        }
        for card in again.cards() {
            self.apply(BoardEvent::Take(card.id.clone())).await;
        }
    }

    /// Moves the board by `event`, writes the records that changed, tells the person what is worth
    /// telling, starts and stops the tasks the board asks for, and returns the effects.
    pub async fn apply(&self, event: BoardEvent) -> Vec<BoardEffect> {
        self.apply_saying(event, None).await
    }

    async fn apply_saying(&self, event: BoardEvent, detail: Option<String>) -> Vec<BoardEffect> {
        let (before, after, effects) = self.with_state(|state| {
            let before = state.board.clone();
            let (board, effects) = step(std::mem::take(&mut state.board), event);
            state.board = board.clone();
            (before, board, effects)
        });
        let saved = effects.iter().filter_map(|effect| match effect {
            BoardEffect::Save(record) => Some(record.clone()),
            _ => None,
        });
        let groups = by_space(saved, &before, &after);
        if !groups.is_empty() {
            let service = self.inner.service.lock().await;
            for (space, records) in &groups {
                service.write_helper_records(space, records).await;
            }
        }
        for effect in &effects {
            match effect {
                BoardEffect::Say {
                    helper,
                    name,
                    notice,
                } => self.tell_notice(helper, name, notice, detail.as_deref()),
                BoardEffect::Launch(launch) => self.launch(launch),
                BoardEffect::Stop(at) => self.stop(at),
                BoardEffect::Save(_)
                | BoardEffect::Created(_)
                | BoardEffect::Posted(_)
                | BoardEffect::JobRefused(_) => {}
            }
        }
        self.keep_conversations().await;
        if !effects.is_empty() {
            self.inner.shared.surface().changed(Change::HelpersChanged);
        }
        effects
    }

    /// The open watch sessions whose agent app a terminal named, as the router lists them; none
    /// when it does not answer.
    pub(super) async fn watched_sessions(&self) -> Vec<docket_core::WatchedLine> {
        self.inner
            .service
            .lock()
            .await
            .intents
            .checkpoint_watched()
            .await
            .unwrap_or_default()
    }

    /// The session the person talks with `helper` in, when it has one.
    pub fn conversation_of(&self, helper: &HelperId) -> Option<SessionId> {
        self.with_state(|state| state.conversations.get(helper).map(|c| c.session.clone()))
    }

    /// Gives each helper on the board its one conversation (a session with the helper's seat, which
    /// an editor may speak in) and ends the conversation of a helper that is gone. One pass at a
    /// time, under the service companion's lock, so a helper never gets two.
    async fn keep_conversations(&self) {
        let service = self.inner.service.lock().await;
        let (missing, gone) = self.with_state(|state| {
            let missing: Vec<HelperCard> = state
                .board
                .cards()
                .iter()
                .filter(|card| !state.conversations.contains_key(&card.id))
                .cloned()
                .collect();
            let gone: Vec<(HelperId, SessionId)> = state
                .conversations
                .iter()
                .filter(|(helper, _)| state.board.card(helper).is_none())
                .map(|(helper, talk)| (helper.clone(), talk.session.clone()))
                .collect();
            (missing, gone)
        });
        for card in missing {
            let open = SessionOpen {
                space: card.space.clone(),
                agent: AgentRef::Companion,
                parent: None,
                cwd: None,
                started_from: None,
                external: None,
                helper: Some(HelperSeat {
                    helper: card.id.clone(),
                    name: card.name.clone(),
                    reach: card.reach.clone(),
                    owns: Vec::new(),
                    place: SeatPlace::Conversation,
                }),
            };
            match service.intents.session_open(open).await {
                Ok(opened) => self.with_state(|state| {
                    state.conversations.insert(
                        card.id.clone(),
                        OpenedJob {
                            task: opened.task,
                            session: opened.session,
                        },
                    );
                }),
                Err(cause) => {
                    tracing::warn!(%cause, "a helper's conversation could not be opened");
                }
            }
        }
        for (helper, session) in gone {
            self.with_state(|state| state.conversations.remove(&helper));
            self.with_talks(|talks| talks.remove(&helper));
            if let Err(cause) = service.intents.session_close(session.clone()).await {
                tracing::warn!(%session, %cause, "a helper's conversation would not close");
            }
        }
    }

    fn tell_notice(
        &self,
        helper: &HelperId,
        name: &HelperName,
        notice: &Notice,
        detail: Option<&str>,
    ) {
        // A failed job whose model said why is told with that sentence, which names the model.
        let text = match (notice, detail) {
            (Notice::Failed(_), Some(why)) => {
                format!("{} stopped. {}.", name.as_str(), why.trim_end_matches('.'))
            }
            _ => notice.plain(name),
        };
        self.tell(helper, text);
    }

    fn tell(&self, helper: &HelperId, text: String) {
        tracing::info!(%text, "a helper");
        self.with_state(|state| {
            state.said = state.said.saturating_add(1);
            state.told.push_back(Told {
                n: state.said,
                helper: helper.clone(),
                text,
            });
            while state.told.len() > MOST_NOTICES {
                state.told.pop_front();
            }
        });
    }

    /// The job as a run needs it: its card, the job, and the answers it is handed that are still
    /// held.
    fn spec_of(&self, launch: &Launch) -> Option<JobSpec> {
        self.with_state(|state| {
            let card = state.board.card(&launch.helper)?.clone();
            let job = state.board.job(launch.job)?.clone();
            let inputs = job
                .with
                .iter()
                .filter_map(|earlier| state.answers.get(earlier))
                .map(|a| JobInput {
                    from: a.session.clone(),
                    text: a.words.clone(),
                })
                .collect();
            Some(JobSpec { card, job, inputs })
        })
    }

    /// Starts the task that runs a job, and the task that watches it.
    fn launch(&self, launch: &Launch) {
        let Some(spec) = self.spec_of(launch) else {
            return;
        };
        let job = launch.job;
        self.with_state(|state| {
            state.running.insert(
                job,
                Running {
                    helper: launch.helper.clone(),
                    abort: None,
                    task: None,
                    session: None,
                    shared: None,
                },
            );
        });
        let hub = self.clone();
        let work = tokio::spawn(async move { hub.work(spec).await });
        let abort = work.abort_handle();
        self.with_state(|state| {
            if let Some(running) = state.running.get_mut(&job) {
                running.abort = Some(abort);
            }
        });
        let hub = self.clone();
        tokio::spawn(async move {
            let outcome = work.await;
            hub.ended(job, outcome).await;
        });
    }

    /// Gives the hub the host of the agent apps that are helpers. A helper whose kind is an app
    /// has its jobs run there; without one such a job fails at once, and nothing else changes.
    pub fn set_agents(&self, agents: Arc<dyn AgentApps>) {
        // Set once: a second host is ignored rather than swapped under running jobs.
        let _ = self.inner.agents.set(agents);
    }

    pub(super) fn agents(&self) -> Option<Arc<dyn AgentApps>> {
        self.inner.agents.get().cloned()
    }

    /// One job, from its session to its report: the body of the task that runs it.
    async fn work(&self, spec: JobSpec) -> Finished {
        match spec.card.kind.program() {
            Some(_) => self.work_agent(spec).await,
            None => self.work_native(spec).await,
        }
    }

    /// A job of a helper that runs an agent app: the goal is the app's one turn, and what it says
    /// comes back as the job's answer. The app's session at the router is the app process's, so
    /// the hub never closes it. A job that cannot start because the app is not signed in is the
    /// board's `NeedsSignIn`, never a failure.
    async fn work_agent(&self, spec: JobSpec) -> Finished {
        let job = spec.job.id;
        let unstarted = |fault| Finished {
            opened: None,
            report: JobReport::failed(fault),
            hold: Hold::Leave,
        };
        let Some(agents) = self.agents() else {
            return unstarted(JobFault::Crashed);
        };
        // The app has no task of ours; the card still names one, so a pause finds the job.
        let Ok(task) = TaskId::parse(&format!("agent-job-{}", job.0)) else {
            return unstarted(JobFault::Crashed);
        };
        self.with_state(|state| {
            if let Some(running) = state.running.get_mut(&job) {
                running.task = Some(task.clone());
            }
        });
        self.apply(BoardEvent::Started { job, task }).await;
        let (ending, opened) = match agents.run(&spec.card, &spec.job).await {
            AgentOutcome::Done {
                words,
                task,
                session,
                own_session,
            } => {
                self.with_state(|state| match own_session {
                    Some(_) => state.can_hand.insert(spec.card.id.clone()),
                    None => state.can_hand.remove(&spec.card.id),
                });
                (
                    JobEnding::Done {
                        words: words.into_inner(),
                    },
                    Some(OpenedJob { task, session }),
                )
            }
            AgentOutcome::NeedsSignIn(ways) => (JobEnding::NeedsSignIn(ways), None),
            AgentOutcome::Failed(fault) => (JobEnding::Failed(fault), None),
            AgentOutcome::Stopped => (JobEnding::Stopped, None),
        };
        Finished {
            opened,
            report: JobReport {
                ending,
                calls: Vec::new(),
                owned: Vec::new(),
                why: None,
            },
            hold: Hold::Leave,
        }
    }

    async fn work_native(&self, spec: JobSpec) -> Finished {
        let job = spec.job.id;
        let mut companion = self.helpers_companion(&spec.card);
        let opened = match companion.open_job(&spec).await {
            Ok(opened) => opened,
            Err(fault) => {
                return Finished {
                    opened: None,
                    report: JobReport::failed(fault),
                    hold: Hold::Close,
                };
            }
        };
        let shared = companion.shared.clone();
        self.with_state(|state| {
            if let Some(running) = state.running.get_mut(&job) {
                running.task = Some(opened.task.clone());
                running.session = Some(opened.session.clone());
                running.shared = Some(shared);
            }
        });
        self.apply(BoardEvent::Started {
            job,
            task: opened.task.clone(),
        })
        .await;
        let report = companion.run_job(&opened).await;
        Finished {
            opened: Some(opened),
            report,
            hold: Hold::Close,
        }
    }

    /// A job's task ended, by itself or not: tell the board how, and close what is not kept. A
    /// task that panicked ends its job as failed, with a plain sentence on its helper; a task that
    /// was cut off ends it as stopped. Nothing else is touched.
    async fn ended(&self, job: JobId, outcome: Result<Finished, JoinError>) {
        let Some(running) = self.with_state(|state| state.running.remove(&job)) else {
            return;
        };
        let conclusion = match outcome {
            Ok(done) => conclude(job, done),
            Err(join) if join.is_cancelled() => Conclusion {
                event: BoardEvent::Stopped(job),
                ..failure(job, JobFault::Crashed, running.session.clone())
            },
            Err(_) => {
                tracing::error!(%job, "a helper's job panicked; only that job ends");
                failure(job, JobFault::Crashed, running.session.clone())
            }
        };
        let Conclusion {
            event,
            answer,
            close,
            calls,
            owned,
            why,
            ways,
        } = conclusion;
        // An agent app that answered is signed in: the agent said so by opening its session.
        let answered_by_app = answer.as_ref().is_some_and(|a| a.hold == Hold::Leave);
        let evicted = answer
            .map(|answer| self.keep(job, answer))
            .unwrap_or_default();
        if let Some(ways) = ways {
            self.with_state(|state| {
                state.signed.remove(&running.helper);
                state.signins.insert(running.helper.clone(), ways);
            });
        }
        if let Some(why) = &why {
            self.with_state(|state| {
                state.whys.insert(job, why.clone());
                // As many as the answers kept: the oldest go first.
                while state.whys.len() > KEPT_ANSWERS {
                    state.whys.pop_first();
                }
            });
        }
        self.apply_saying(event, why).await;
        if answered_by_app {
            self.signed_in_to_app_of(&running.helper).await;
        }
        for verdict in calls {
            self.apply(BoardEvent::Called {
                helper: running.helper.clone(),
                verdict,
            })
            .await;
        }
        // Each owner is named once, however many of its things were reached for.
        let owners: BTreeSet<HelperName> = self.with_state(|state| {
            owned
                .iter()
                .filter_map(|thing| state.board.owner_of(thing, Some(job)))
                .map(|(card, _)| card.name.clone())
                .collect()
        });
        for owner in owners {
            self.tell(&running.helper, owned_sentence(&owner));
        }
        for session in close.into_iter().chain(evicted) {
            self.close_session(session).await;
        }
    }

    /// The words a finished job answered, while they are kept.
    pub fn answer_words(&self, job: JobId) -> Option<String> {
        self.with_state(|state| state.answers.get(&job).map(|a| a.words.clone()))
    }

    /// What is kept of a job's words: its answer, and why it stopped.
    pub fn job_words(&self, job: JobId) -> companion_wire::JobWords {
        self.with_state(|state| companion_wire::JobWords {
            answer: state.answers.get(&job).map(|a| a.words.clone()),
            why: state.whys.get(&job).cloned(),
        })
    }

    /// The agent app `helper` runs is signed in. It has one login, so every helper that runs the
    /// same app is: those that waited for it take their jobs now.
    pub(super) async fn signed_in_to_app_of(&self, helper: &HelperId) {
        let Some(program) = self.program_of(helper) else {
            return;
        };
        let waiting: Vec<HelperId> = self.with_state(|state| {
            let same: Vec<&HelperCard> = state
                .board
                .cards()
                .iter()
                .filter(|card| card.kind.program() == Some(&program))
                .collect();
            let waiting = same
                .iter()
                .filter(|card| card.state == HelperState::Waiting(Need::SignIn))
                .map(|card| card.id.clone())
                .collect();
            let ids: Vec<HelperId> = same.iter().map(|card| card.id.clone()).collect();
            for id in ids {
                state.signins.remove(&id);
                state.signed.insert(id);
            }
            waiting
        });
        for id in waiting {
            self.apply(BoardEvent::SignedIn(id)).await;
        }
    }

    /// The agent app `helper` runs, if it runs one.
    pub(super) fn program_of(&self, helper: &HelperId) -> Option<AgentProgram> {
        self.with_state(|state| state.board.card(helper)?.kind.program().cloned())
    }

    /// Keeps a finished job's answer for the jobs that are handed it; the oldest answers beyond
    /// the most kept are dropped and their sessions are returned to be closed.
    fn keep(&self, job: JobId, answer: Answer) -> Vec<SessionId> {
        self.with_state(|state| {
            state.answers.insert(job, answer);
            state.kept.push_back(job);
            let mut old = Vec::new();
            while state.kept.len() > KEPT_ANSWERS {
                if let Some(oldest) = state.kept.pop_front()
                    && let Some(gone) = state.answers.remove(&oldest)
                    && gone.hold == Hold::Close
                {
                    old.push(gone.session);
                }
            }
            old
        })
    }

    async fn close_session(&self, session: SessionId) {
        let service = self.inner.service.lock().await;
        let closed = service.intents.session_close(session.clone()).await;
        if let Err(cause) = closed {
            tracing::warn!(%session, %cause, "a helper's session would not close");
        }
    }

    /// Stops a running job: its task is asked to end, and cut off if it has not after a short
    /// while (a call the router already carries out is not abandoned half way).
    fn stop(&self, at: &TaskRef) {
        let found = self.with_state(|state| {
            state.running.get(&at.job).map(|r| {
                (
                    r.shared.clone(),
                    r.task.clone(),
                    r.abort.clone(),
                    r.helper.clone(),
                )
            })
        });
        let Some((shared, task, abort, helper)) = found else {
            return;
        };
        if let (Some(shared), Some(task)) = (shared, task) {
            shared.cancel(&task);
        }
        // A job on an agent app is stopped at the app; for any other helper this finds nothing.
        if let Some(agents) = self.agents() {
            tokio::spawn(async move { agents.stop(&helper).await });
        }
        if let Some(abort) = abort {
            tokio::spawn(async move {
                tokio::time::sleep(GRACE).await;
                abort.abort();
            });
        }
    }
}

/// What the routines need of the hub: where a helper works, what its jobs came to, the sentences
/// the person is told, and the records and events of the Spaces.
impl<P, I> Hub<P, I>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    /// The Space of a helper.
    pub(crate) fn helper_space(&self, helper: &HelperId) -> Option<SpaceId> {
        self.with_state(|state| state.board.card(helper).map(|c| c.space.clone()))
    }

    /// What the person calls a helper.
    pub(crate) fn helper_name(&self, helper: &HelperId) -> Option<HelperName> {
        self.with_state(|state| state.board.card(helper).map(|c| c.name.clone()))
    }

    /// How a job ended, once it has: done, or open again after a failure. A job the board no
    /// longer holds did not finish.
    pub(crate) fn job_end(&self, job: JobId) -> Option<JobEnd> {
        self.with_state(|state| {
            let Some(found) = state.board.job(job) else {
                return Some(JobEnd::Failed(JobFault::Crashed));
            };
            match (&found.status, found.failed) {
                (JobStatus::Done(_), _) => Some(JobEnd::Done),
                (_, Some(fault)) => Some(JobEnd::Failed(fault)),
                _ => None,
            }
        })
    }

    /// Tells the person a sentence about a helper, once, and the shell that something changed.
    pub(crate) fn say(&self, helper: &HelperId, text: String) {
        self.tell(helper, text);
        self.inner.shared.surface().changed(Change::HelpersChanged);
    }

    /// Tells the shell that something about the helpers changed.
    pub(super) fn changed(&self) {
        self.inner.shared.surface().changed(Change::HelpersChanged);
    }

    /// Writes `records` of the Space `space`.
    pub(crate) async fn write_records(&self, space: &SpaceId, records: &[HelperRecord]) {
        self.inner
            .service
            .lock()
            .await
            .write_helper_records(space, records)
            .await;
    }

    /// The routines' records the Spaces hold.
    pub(crate) async fn read_routine_records(&self, spaces: &[SpaceId]) -> Vec<HelperRecord> {
        self.inner
            .service
            .lock()
            .await
            .read_routine_records(spaces)
            .await
    }

    /// The events `query` asks for in `space`, newest first, through a session opened there for
    /// the purpose. A router that does not answer gives none.
    pub(crate) async fn recent_in(&self, space: &SpaceId, query: RecentQuery) -> Vec<RecentEntry> {
        let service = self.inner.service.lock().await;
        let opened = service
            .intents
            .session_open(SessionOpen {
                space: space.clone(),
                agent: AgentRef::Companion,
                parent: None,
                cwd: None,
                started_from: None,
                external: None,
                helper: None,
            })
            .await;
        let Ok(opened) = opened else {
            tracing::warn!(?space, "no session to read events through");
            return Vec::new();
        };
        let found = RouterRecent::new(&service.intents, opened.session.clone())
            .recent(query)
            .await;
        let closed = service.intents.session_close(opened.session.clone()).await;
        if let Err(cause) = closed {
            tracing::warn!(session = %opened.session, %cause, "a session would not close");
        }
        found.unwrap_or_default()
    }
}
