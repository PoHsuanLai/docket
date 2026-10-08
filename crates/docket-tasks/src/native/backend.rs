//! `NativeBackend`: one session's turns run by the planner loop of a `Companion`, behind the
//! `SessionBackend` trait. The backend holds no authority and writes no entry: every call goes to
//! the router, which gates it and records it, and the host writes the rest.
//!
//! Two rules of the trait are kept here and tested (`tests/it/native_contract.rs`):
//! `next_event` is cancel-safe (the turn lives in a stored `Flight`; a dropped pull loses and
//! repeats nothing), and a call is not made until the pull after the one that returned its
//! `Started` event, so a reader that stops the turn in between stops the call before it runs.

use crate::native::end::end_of;
use crate::native::flight::Flight;
use crate::runtime::Companion;
use crate::seams::{Now, Surface};
use crate::shared::Shared;
use companion_wire::AskWire;
use docket_client::Transport as IntentsTransport;
use docket_core::{ContextKeep, Keep, TurnIn, UserTurn, WindowKey};
use docket_session::{
    BackendEvent, BackendFault, BackendKind, ResumePlan, Resumed, SessionBackend, Standing,
    StartSession, TurnEnd,
};
use futures_util::FutureExt;
use futures_util::lock::Mutex;
use porter_client::Transport as InferTransport;
use prov::{SessionId, TaskId};
use std::sync::Arc;

/// The companion the native backends of a host share. A turn holds it while it runs, as
/// companiond's own lock is held for a turn: sessions on one companion take turns.
pub type Core<P, I, K, S> = Arc<Mutex<Companion<P, I, K, S>>>;

/// Where the window of an editor's session is anchored: nowhere in particular.
const EDITOR_WINDOW: &str = "editor";

/// Whether the backend has a task to run turns on.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Bound {
    /// Not started.
    No,
    /// Running turns for this task.
    Task(TaskId),
    /// Closed, or resumed for display only.
    Over,
}

/// One session's backend.
#[derive(Debug)]
pub struct NativeBackend<P: InferTransport, I: IntentsTransport, K, S: Surface> {
    core: Core<P, I, K, S>,
    shared: Arc<Shared<S>>,
    session: SessionId,
    bound: Bound,
    flight: Option<Flight>,
}

/// The chips of context an editor's turn keeps: none.
fn kept_nothing() -> ContextKeep {
    ContextKeep {
        query: Keep::Dropped,
        results: Keep::Dropped,
        selection: Keep::Dropped,
        window: Keep::Dropped,
    }
}

impl<P, I, K, S> NativeBackend<P, I, K, S>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
    K: Now + Send + Sync + 'static,
    S: Surface + Send + Sync + 'static,
{
    /// The backend of `session`, which runs on `core`.
    pub fn new(core: Core<P, I, K, S>, shared: Arc<Shared<S>>, session: SessionId) -> Self {
        Self {
            core,
            shared,
            session,
            bound: Bound::No,
            flight: None,
        }
    }

    /// Takes up the task the companion already runs on this session.
    async fn bind(&mut self) -> Result<(), BackendFault> {
        let core = self.core.lock().await;
        let task = core
            .task_of(&self.session)
            .ok_or(BackendFault::Unavailable)?;
        self.bound = Bound::Task(task);
        Ok(())
    }
}

/// Whether the router already numbered the person's turn.
#[derive(Debug, Clone)]
enum Recording {
    /// It did: the turn carries its id.
    Done,
    /// It has not: the turn's flight records it first, so a sheet the router asks while deriving
    /// the turn's policy is shown by the same pulls that run the turn.
    Pending(TurnIn),
}

impl<P, I, K, S> NativeBackend<P, I, K, S>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
    K: Now + Send + Sync + 'static,
    S: Surface + Send + Sync + 'static,
{
    /// Records `said` with the router (which numbers it and derives the task's policy, and may
    /// ask the person about it) and runs the turn, all inside the flight.
    pub fn record_and_turn(&mut self, turn: UserTurn, said: TurnIn) -> Result<(), BackendFault> {
        self.begin(turn, Recording::Pending(said))
    }

    fn begin(&mut self, turn: UserTurn, recording: Recording) -> Result<(), BackendFault> {
        let Bound::Task(task) = self.bound.clone() else {
            return Err(BackendFault::NotRunning);
        };
        if self.flight.is_some() {
            return Err(BackendFault::Busy);
        }
        let window = WindowKey::parse(EDITOR_WINDOW).map_err(|_| BackendFault::Unavailable)?;
        let core = self.core.clone();
        let session = self.session.clone();
        self.flight = Some(Flight::of(|mut tap| {
            async move {
                let mut core = core.lock().await;
                let mut turn = turn;
                if let Recording::Pending(said) = recording {
                    match core.intents.session_turn(session.clone(), said).await {
                        Ok(id) => turn.id = id,
                        Err(_) => return TurnEnd::Failed,
                    }
                }
                core.reopen(&task);
                let Ok(begun) = core.begin_ask(AskWire {
                    session,
                    turn,
                    keep: kept_nothing(),
                    parent_window: window,
                    app: None,
                }) else {
                    return TurnEnd::Failed;
                };
                match core.run_begun_tapped(begun, &mut tap).await {
                    Ok(()) => end_of(
                        core.tasks.get(&task),
                        core.runtimes.get(&task).and_then(|rt| rt.failure.as_ref()),
                    ),
                    Err(_) => TurnEnd::Failed,
                }
            }
            .boxed()
        }));
        Ok(())
    }
}

impl<P, I, K, S> SessionBackend for NativeBackend<P, I, K, S>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
    K: Now + Send + Sync + 'static,
    S: Surface + Send + Sync + 'static,
{
    fn kind(&self) -> BackendKind {
        BackendKind::Native
    }

    async fn start(&mut self, _open: StartSession) -> Result<(), BackendFault> {
        self.bind().await
    }

    async fn resume(&mut self, plan: &ResumePlan) -> Result<Resumed, BackendFault> {
        if !matches!(plan.standing, Standing::Open | Standing::Paused(_)) {
            self.bound = Bound::Over;
            return Ok(Resumed::Restored);
        }
        {
            let mut core = self.core.lock().await;
            if core.task_of(&self.session).is_none() {
                core.adopt_stored(&self.session, plan)
                    .await
                    .map_err(|_| BackendFault::Unavailable)?;
            }
        }
        self.bind().await?;
        Ok(Resumed::Restored)
    }

    async fn turn(&mut self, turn: UserTurn) -> Result<(), BackendFault> {
        self.begin(turn, Recording::Done)
    }

    async fn next_event(&mut self) -> Option<BackendEvent> {
        let flight = self.flight.as_mut()?;
        let event = flight.next().await;
        if matches!(event, Some(BackendEvent::TurnEnd(_)) | None) {
            self.flight = None;
        }
        event
    }

    async fn cancel(&mut self) {
        let (Bound::Task(task), Some(flight)) = (&self.bound, self.flight.as_mut()) else {
            return;
        };
        self.shared.cancel(task);
        flight.stop();
    }

    async fn close(&mut self) {
        self.flight = None;
        self.bound = Bound::Over;
    }
}
