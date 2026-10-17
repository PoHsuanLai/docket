//! The hub: the one owner of the board.

use crate::Companiond;
use crate::Shared;
use docket_client::Transport as IntentsTransport;
use docket_core::helper::{
    Board, BoardEffect, BoardEvent, CardRecord, HelperName, HelperRecord, Notice, step,
};
use futures_util::lock::Mutex as AsyncMutex;
use porter_client::Transport as InferTransport;
use prov::{HelperId, SpaceId};
use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};

/// The most notices kept for the shell to read.
const MOST_NOTICES: usize = 20;

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

#[derive(Debug, Default)]
struct State {
    board: Board,
    told: VecDeque<Told>,
    said: u64,
}

struct Inner<P: InferTransport, I: IntentsTransport> {
    state: Mutex<State>,
    /// Reads and writes the records through the router. Nothing else uses it, so a record is
    /// never written behind a person's turn.
    service: AsyncMutex<Companiond<P, I>>,
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

impl<P, I> Hub<P, I>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    /// A hub with an empty board. `service` is a companion of its own that reads and writes the
    /// helpers' records; `shared` is the front companion's view, where changes are told.
    pub fn new(service: Companiond<P, I>, shared: Arc<Shared>) -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State::default()),
                service: AsyncMutex::new(service),
                shared,
            }),
        }
    }

    fn with_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        match self.inner.state.lock() {
            Ok(mut state) => f(&mut state),
            Err(poisoned) => f(&mut poisoned.into_inner()),
        }
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
    /// telling, and returns the effects the caller carries out.
    pub async fn apply(&self, event: BoardEvent) -> Vec<BoardEffect> {
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
            if let BoardEffect::Say {
                helper,
                name,
                notice,
            } = effect
            {
                self.tell(helper, notice, name);
            }
        }
        effects
    }

    fn tell(&self, helper: &HelperId, notice: &Notice, name: &HelperName) {
        let text = notice.plain(name);
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
}
