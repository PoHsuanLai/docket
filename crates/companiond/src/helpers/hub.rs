//! The hub: the one owner of the board, and the place its jobs run from.
//!
//! A job runs as a tokio task of its own on a `Companion` of its own (built by the factory the
//! daemon passes in), so several helpers work at once, none behind the front conversation's lock,
//! and a panic or an error in one job ends that job: a second task awaits the first and turns
//! what it finds, a result or a `JoinError`, into the board's event for that job. The daemon and
//! every other job go on.

use crate::Companiond;
use crate::Shared;
use docket_client::Transport as IntentsTransport;
use docket_core::ValidManifest;
use docket_core::helper::{
    Board, BoardEffect, BoardEvent, CallVerdict, CardRecord, HelperCard, HelperName, HelperRecord,
    JobFault, JobId, Launch, Notice, ResultHandle, TaskRef, owned_sentence, step,
};
use docket_tasks::{Change, JobEnding, JobInput, JobReport, JobSpec, OpenedJob, Surface};
use futures_util::lock::Mutex as AsyncMutex;
use porter_client::Transport as InferTransport;
use prov::{EntityId, HelperId, SessionId, SpaceId, TaskId};
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

/// What a finished job's answer is, while the process lives: the session it was given in and its
/// words. It is never written down: a handle to it expires with the session.
#[derive(Debug, Clone)]
struct Answer {
    session: SessionId,
    words: String,
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
    /// The front companion's shared view: where the shell is told something changed.
    shared: Arc<Shared>,
}

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
}

/// What a job's end means for the board and for the router.
struct Conclusion {
    event: BoardEvent,
    answer: Option<Answer>,
    close: Option<SessionId>,
    calls: Vec<CallVerdict>,
    owned: Vec<EntityId>,
    why: Option<String>,
}

fn failure(job: JobId, fault: JobFault, close: Option<SessionId>) -> Conclusion {
    Conclusion {
        event: BoardEvent::Failed { job, fault },
        answer: None,
        close,
        calls: Vec::new(),
        owned: Vec::new(),
        why: None,
    }
}

/// How a job's end is told to the board. A run that ended in a words answer keeps its session open
/// for its answer to be handed on; every other end closes it.
fn conclude(job: JobId, done: Finished) -> Conclusion {
    let Finished { opened, report } = done;
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
            }),
            close: None,
            calls: Vec::new(),
            owned: Vec::new(),
            why: None,
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
        if !effects.is_empty() {
            self.inner.shared.surface().changed(Change::HelpersChanged);
        }
        effects
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

    /// One job, from its session to its report: the body of the task that runs it.
    async fn work(&self, spec: JobSpec) -> Finished {
        let job = spec.job.id;
        let mut companion = (self.inner.make)(&spec.card);
        let opened = match companion.open_job(&spec).await {
            Ok(opened) => opened,
            Err(fault) => {
                return Finished {
                    opened: None,
                    report: JobReport::failed(fault),
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
        } = conclusion;
        let evicted = answer
            .map(|answer| self.keep(job, answer))
            .unwrap_or_default();
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
            state
                .running
                .get(&at.job)
                .map(|r| (r.shared.clone(), r.task.clone(), r.abort.clone()))
        });
        let Some((shared, task, abort)) = found else {
            return;
        };
        if let (Some(shared), Some(task)) = (shared, task) {
            shared.cancel(&task);
        }
        if let Some(abort) = abort {
            tokio::spawn(async move {
                tokio::time::sleep(GRACE).await;
                abort.abort();
            });
        }
    }
}
