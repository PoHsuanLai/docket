//! The hub: the one owner of the board. It carries the board's effects out: the records written
//! through the router (and read back at a restart), the notices the person is shown, and each
//! helper's conversation. Running the jobs is `jobs.rs`; what the routines need of it is
//! `routine_link.rs`; the decisions are `companion_core`'s.

use crate::spawn::{Cutoff, Spawn};
use companion_core::{Answers, Logins, OpenedJob, Proposals, Terminals, Told, ToldLog, by_space};
use docket_client::Transport as IntentsTransport;
use docket_core::helper::{
    AgentApps, Board, BoardEffect, BoardEvent, HelperCard, HelperName, HelperSeat, JobId, Notice,
    SeatPlace, step,
};
use docket_core::{SessionOpen, ValidManifest};
use docket_planner::Planner;
use docket_tasks::{Change, Companion, Shared, Surface};
use docket_vocab::Clock;
use futures_util::lock::Mutex as AsyncMutex;
use prov::{AgentRef, HelperId, SessionId, SpaceId, TaskId};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};

/// A job that is running: how to stop it, and where it runs.
pub(super) struct Running<S: Surface> {
    pub(super) helper: HelperId,
    pub(super) cutoff: Option<Cutoff>,
    pub(super) task: Option<TaskId>,
    pub(super) session: Option<SessionId>,
    pub(super) shared: Option<Arc<Shared<S>>>,
}

/// What the hub keeps beside the board. The kept answers, the sentences told, the logins, the
/// proposals and where the conversations are come from `companion_core`; this holds them with the
/// tasks that are running.
#[derive(Default)]
pub(super) struct State<S: Surface> {
    pub(super) board: Board,
    pub(super) told: ToldLog,
    pub(super) running: BTreeMap<JobId, Running<S>>,
    pub(super) answers: Answers,
    pub(super) logins: Logins,
    /// The one session each helper has for the person to talk with it in, kept until the helper
    /// is removed. Not across a restart: the next pass opens it again.
    conversations: BTreeMap<HelperId, OpenedJob>,
    /// The routines proposed that wait for the person. Not across a restart.
    pub(super) proposals: Proposals,
    pub(super) terminals: Terminals,
}

/// What builds the companion a job runs on: its transports are the daemon's, its planner opens its
/// sessions with `docket_tasks::open_options` of the card.
type Make<P, I, K, S> = dyn Fn(&HelperCard) -> Companion<P, I, K, S> + Send + Sync;

pub(super) struct Inner<P: Planner, I: IntentsTransport, K: Clock, S: Surface> {
    pub(super) state: Mutex<State<S>>,
    /// Reads and writes the records through the router. Nothing else uses it, so a record is
    /// never written behind a person's turn.
    pub(super) service: AsyncMutex<Companion<P, I, K, S>>,
    make: Box<Make<P, I, K, S>>,
    /// The companion each helper's conversation runs its turns on, made when the person first
    /// speaks and kept, so a turn sees the ones before it.
    talks: Mutex<BTreeMap<HelperId, Talk<P, I, K, S>>>,
    /// The host of the agent apps that are helpers, when the daemon has one.
    pub(super) agents: std::sync::OnceLock<Arc<dyn AgentApps>>,
    /// The front companion's shared view: where the shell is told something changed.
    pub(super) shared: Arc<Shared<S>>,
    /// How a job's task is started and cut off.
    pub(super) spawn: Arc<dyn Spawn>,
}

/// The companion one helper's conversation runs its turns on.
type Talk<P, I, K, S> = Arc<AsyncMutex<Companion<P, I, K, S>>>;

/// The board and what carries it out. Cheap to clone: every clone is the same board.
pub struct Hub<P: Planner, I: IntentsTransport, K: Clock, S: Surface> {
    pub(super) inner: Arc<Inner<P, I, K, S>>,
}

impl<P: Planner, I: IntentsTransport, K: Clock, S: Surface> Clone for Hub<P, I, K, S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<P: Planner, I: IntentsTransport, K: Clock, S: Surface> fmt::Debug for Hub<P, I, K, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hub").finish_non_exhaustive()
    }
}

impl<P, I, K, S> Hub<P, I, K, S>
where
    P: Planner + 'static,
    I: IntentsTransport + 'static,
    K: Clock + 'static,
    S: Surface + Send + Sync + 'static,
{
    /// A hub with an empty board. `service` is a companion of its own that reads and writes the
    /// helpers' records; `shared` is the front companion's view, where changes are told; `spawn`
    /// starts the tasks that run the jobs; `make` builds the companion a helper's job runs on.
    pub fn new(
        service: Companion<P, I, K, S>,
        shared: Arc<Shared<S>>,
        spawn: Arc<dyn Spawn>,
        make: impl Fn(&HelperCard) -> Companion<P, I, K, S> + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State::default()),
                service: AsyncMutex::new(service),
                make: Box::new(make),
                talks: Mutex::new(BTreeMap::new()),
                agents: std::sync::OnceLock::new(),
                shared,
                spawn,
            }),
        }
    }

    pub(super) fn with_state<R>(&self, f: impl FnOnce(&mut State<S>) -> R) -> R {
        match self.inner.state.lock() {
            Ok(mut state) => f(&mut state),
            Err(poisoned) => f(&mut poisoned.into_inner()),
        }
    }

    fn with_talks<R>(&self, f: impl FnOnce(&mut BTreeMap<HelperId, Talk<P, I, K, S>>) -> R) -> R {
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
    pub(super) fn helpers_companion(&self, card: &HelperCard) -> Companion<P, I, K, S> {
        let mut companion = (self.inner.make)(card);
        companion.set_desk(Arc::new(self.clone()));
        companion
    }

    /// The companion that runs the turns of `card`'s conversation, made the first time, on the
    /// planner and transports the daemon gives that helper.
    pub(super) fn talker(&self, card: &HelperCard) -> Talk<P, I, K, S> {
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
        self.with_state(|state| state.told.since(after))
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

    pub(super) async fn apply_saying(
        &self,
        event: BoardEvent,
        detail: Option<String>,
    ) -> Vec<BoardEffect> {
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
                acts_for: None,
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

    pub(super) fn tell(&self, helper: &HelperId, text: String) {
        tracing::info!(%text, "a helper");
        self.with_state(|state| state.told.tell(helper, text));
    }

    /// Tells the shell that something about the helpers changed.
    pub(super) fn changed(&self) {
        self.inner.shared.surface().changed(Change::HelpersChanged);
    }
}
