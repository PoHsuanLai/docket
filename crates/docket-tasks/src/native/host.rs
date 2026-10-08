//! `NativeHost`: the registry of native sessions over one companion. It answers the edges (the ACP
//! server, `quire-do`, a launcher) through `SessionHost`. The router writes the log of a native
//! session as it goes (opening, turns, calls, steps, taint, close), so the host appends nothing
//! to a session that runs; it reads the log to resume, export and fork, and appends only the
//! first entries of a fork, which the router then restores by name like any stored session.

use crate::native::backend::{Core, NativeBackend};
use crate::runtime::Companion;
use crate::seams::{Now, Surface};
use crate::shared::Shared;
use companion_wire::NeedsYou;
use docket_client::Transport as IntentsTransport;
use docket_core::{
    ConfirmId, ConfirmRequest, ContextKeep, Keep, Origin, SessionOpen, TurnIn, UserTurn,
};
use docket_session::{
    BackendEvent, BackendFault, EndCause, HostFault, NoDesk, Opening, ResumePlan, Seq,
    SessionBackend, SessionExport, SessionHost, SessionLog, SheetChoice, SheetDesk, Standing,
    StartSession, child_names, export, fork, forks_of, read_all, resume_plan,
};
use futures_util::future::{Either, select};
use futures_util::lock::Mutex;
use porter_client::Transport as InferTransport;
use prov::{AgentRef, SessionId};
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::sync::Arc;

/// What a pull of a session found.
enum Pulled {
    /// A sheet the desk was handed.
    Sheet(Box<ConfirmRequest>),
    /// The desk holds no sheets at all (not an editor's host).
    NoDesk,
    /// The backend's next event.
    Event(Option<BackendEvent>),
}

/// The native sessions of one companion, over the log the router writes.
#[derive(Debug)]
pub struct NativeHost<P: InferTransport, I: IntentsTransport, K, S: Surface, L, D = NoDesk> {
    core: Core<P, I, K, S>,
    shared: Arc<Shared<S>>,
    log: L,
    desk: D,
    backends: BTreeMap<SessionId, NativeBackend<P, I, K, S>>,
    /// Sessions that take no turn: closed, or stored for display only.
    over: BTreeSet<SessionId>,
    /// The sheet each session was told about and whose request has not reached the desk yet.
    awaited: BTreeMap<SessionId, ConfirmId>,
    /// An event pulled while a sheet was awaited and not yet handed out.
    later: BTreeMap<SessionId, Option<BackendEvent>>,
}

impl<P, I, K, S, L> NativeHost<P, I, K, S, L>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
    K: Now + Send + Sync + 'static,
    S: Surface + Send + Sync + 'static,
    L: SessionLog,
{
    /// A host over `companion`, reading `log` (the one the router writes).
    pub fn over(companion: Companion<P, I, K, S>, log: L) -> Self {
        let shared = companion.shared.clone();
        Self::sharing(Arc::new(Mutex::new(companion)), shared, log)
    }

    /// A host over a companion others share.
    pub fn sharing(core: Core<P, I, K, S>, shared: Arc<Shared<S>>, log: L) -> Self {
        Self {
            core,
            shared,
            log,
            desk: NoDesk,
            backends: BTreeMap::new(),
            over: BTreeSet::new(),
            awaited: BTreeMap::new(),
            later: BTreeMap::new(),
        }
    }

    /// The same host with `desk` holding the sheets the router puts to its editor sessions.
    pub fn with_desk<D2: SheetDesk>(self, desk: D2) -> NativeHost<P, I, K, S, L, D2> {
        NativeHost {
            core: self.core,
            shared: self.shared,
            log: self.log,
            desk,
            backends: self.backends,
            over: self.over,
            awaited: self.awaited,
            later: self.later,
        }
    }
}

impl<P, I, K, S, L, D> NativeHost<P, I, K, S, L, D>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
    K: Now + Send + Sync + 'static,
    S: Surface + Send + Sync + 'static,
    L: SessionLog,
    D: SheetDesk,
{
    fn backend(
        &mut self,
        session: &SessionId,
    ) -> Result<&mut NativeBackend<P, I, K, S>, HostFault> {
        self.backends
            .get_mut(session)
            .ok_or(HostFault::NoSuchSession)
    }

    /// The next thing a session shows: a sheet its desk was handed, or the backend's next event.
    /// Cancel-safe: the two futures lose nothing when dropped.
    async fn pull(&mut self, session: &SessionId) -> Result<Pulled, HostFault> {
        let backend = self
            .backends
            .get_mut(session)
            .ok_or(HostFault::NoSuchSession)?;
        let sheet = Box::pin(self.desk.next_sheet(session));
        let event = Box::pin(backend.next_event());
        Ok(match select(sheet, event).await {
            Either::Left((Some(request), _)) => Pulled::Sheet(Box::new(request)),
            Either::Left((None, _)) => Pulled::NoDesk,
            Either::Right((event, _)) => Pulled::Event(event),
        })
    }

    fn fresh(&self, session: &SessionId) -> NativeBackend<P, I, K, S> {
        NativeBackend::new(self.core.clone(), self.shared.clone(), session.clone())
    }
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

impl<P, I, K, S, L, D> SessionHost for NativeHost<P, I, K, S, L, D>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
    K: Now + Send + Sync + 'static,
    S: Surface + Send + Sync + 'static,
    L: SessionLog,
    D: SheetDesk,
{
    async fn open(&mut self, opening: Opening) -> Result<SessionId, HostFault> {
        let opened = {
            let mut core = self.core.lock().await;
            core.open(SessionOpen {
                space: opening.space.clone(),
                agent: opening.agent.clone().unwrap_or(AgentRef::Companion),
                parent: opening.parent.clone(),
                cwd: opening.cwd.clone(),
                started_from: None,
                external: None,
            })
            .await
            .map_err(|_| BackendFault::Unavailable)?
        };
        let mut backend = self.fresh(&opened.session);
        backend
            .start(StartSession {
                session: opened.session.clone(),
                opening,
            })
            .await?;
        self.backends.insert(opened.session.clone(), backend);
        Ok(opened.session)
    }

    async fn resume(&mut self, session: &SessionId) -> Result<ResumePlan, HostFault> {
        let rows = read_all(&self.log, session).await?;
        let plan = resume_plan(&rows)?;
        let mut backend = self
            .backends
            .remove(session)
            .unwrap_or_else(|| self.fresh(session));
        let resumed = backend.resume(&plan).await;
        self.backends.insert(session.clone(), backend);
        resumed?;
        if matches!(plan.standing, Standing::Closed(_) | Standing::Blocked(_)) {
            self.over.insert(session.clone());
        }
        Ok(plan)
    }

    async fn turn(&mut self, session: &SessionId, turn: UserTurn) -> Result<(), HostFault> {
        if self.over.contains(session) {
            return Err(HostFault::NotOpen);
        }
        self.backend(session)?;
        // The router records what the person said, and numbers it, inside the turn's flight.
        let said = TurnIn {
            text: turn.text.clone(),
            origin: Origin::InWindowField,
            keep: kept_nothing(),
            via: turn.via,
        };
        self.backend(session)?.record_and_turn(turn, said)?;
        Ok(())
    }

    /// The router tells the host a sheet is up (`NeedsYou::Confirm`) by one route and the desk is
    /// handed the sheet by another, so either may come first. A sheet the desk holds is shown as
    /// itself, once; one that has not arrived is awaited together with the backend's next event,
    /// and if that comes first the sheet went elsewhere (the desktop) and stays text. All state
    /// between pulls is in fields (`awaited`, `later`), so a dropped pull loses nothing.
    async fn next_event(&mut self, session: &SessionId) -> Result<Option<BackendEvent>, HostFault> {
        loop {
            let pulled = match self.later.remove(session) {
                Some(event) => Pulled::Event(event),
                None => self.pull(session).await?,
            };
            let owed = self.awaited.get(session).cloned();
            let event = match (pulled, owed) {
                (Pulled::Sheet(request), _) => {
                    self.awaited.remove(session);
                    return Ok(Some(BackendEvent::Sheet(request)));
                }
                (Pulled::NoDesk, Some(id)) => {
                    self.awaited.remove(session);
                    return Ok(Some(BackendEvent::NeedsYou(NeedsYou::Confirm(id))));
                }
                (Pulled::NoDesk, None) => self.backend(session)?.next_event().await,
                (Pulled::Event(event), Some(id)) => {
                    // The event came before the sheet: it went elsewhere.
                    self.awaited.remove(session);
                    self.later.insert(session.clone(), event);
                    return Ok(Some(BackendEvent::NeedsYou(NeedsYou::Confirm(id))));
                }
                (Pulled::Event(event), None) => event,
            };
            match event {
                Some(BackendEvent::NeedsYou(NeedsYou::Confirm(id))) => {
                    // On the desk: shown (or about to be) as a sheet. Not yet: wait for it.
                    if self.desk.request(&id).is_none() {
                        self.awaited.insert(session.clone(), id);
                    }
                }
                other => return Ok(other),
            }
        }
    }

    async fn answer_sheet(
        &mut self,
        session: &SessionId,
        id: &ConfirmId,
        choice: SheetChoice,
    ) -> Result<(), HostFault> {
        self.backend(session)?;
        Ok(self.desk.answer(id, choice)?)
    }

    async fn cancel(&mut self, session: &SessionId) -> Result<(), HostFault> {
        self.backend(session)?.cancel().await;
        Ok(())
    }

    async fn close(&mut self, session: &SessionId, _cause: EndCause) -> Result<(), HostFault> {
        self.backend(session)?.close().await;
        self.over.insert(session.clone());
        let mut core = self.core.lock().await;
        core.close(session.clone())
            .await
            .map_err(|_| BackendFault::Unavailable)?;
        Ok(())
    }

    async fn fork(&mut self, session: &SessionId, at: Seq) -> Result<SessionId, HostFault> {
        let rows = read_all(&self.log, session).await?;
        let parent = resume_plan(&rows)?.opening.task;
        let taken = forks_of(session, &self.log.sessions().await?);
        let (child, task) = child_names(session, &parent, taken).ok_or(HostFault::NoSuchSession)?;
        let entries = fork(session, &rows, at, task)?;
        for (n, entry) in entries.iter().enumerate() {
            self.log.append(&child, Seq(n as u64), entry).await?;
        }
        Ok(child)
    }

    fn export(
        &self,
        session: &SessionId,
    ) -> impl Future<Output = Result<SessionExport, HostFault>> + Send {
        // Borrows the log alone: a host with a turn in flight is not `Sync`.
        let log = &self.log;
        async move {
            let rows = read_all(log, session).await?;
            Ok(export(session, &rows)?)
        }
    }
}
