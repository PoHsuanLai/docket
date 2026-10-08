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
use docket_core::{ConfirmId, ContextKeep, Keep, Origin, SessionOpen, TurnIn, UserTurn};
use docket_session::{
    BackendEvent, BackendFault, EndCause, HostFault, NoDesk, Opening, ResumePlan, Seq,
    SessionBackend, SessionExport, SessionHost, SessionLog, SheetChoice, SheetDesk, Standing,
    StartSession, export, fork, read_all, resume_plan,
};
use futures_util::lock::Mutex;
use porter_client::Transport as InferTransport;
use prov::{AgentRef, SessionId, TaskId};
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::sync::Arc;

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
        // The router records what the person said, and numbers it.
        let id = {
            let core = self.core.lock().await;
            core.intents
                .session_turn(
                    session.clone(),
                    TurnIn {
                        text: turn.text.clone(),
                        origin: Origin::InWindowField,
                        keep: kept_nothing(),
                        via: turn.via,
                    },
                )
                .await
                .map_err(|_| BackendFault::Unavailable)?
        };
        self.backend(session)?.turn(UserTurn { id, ..turn }).await?;
        Ok(())
    }

    async fn next_event(&mut self, session: &SessionId) -> Result<Option<BackendEvent>, HostFault> {
        let event = self.backend(session)?.next_event().await;
        // A sheet the desk holds goes to the edge as itself; one it does not hold stays text.
        Ok(match event {
            Some(BackendEvent::NeedsYou(NeedsYou::Confirm(id))) => {
                Some(match self.desk.request(&id) {
                    Some(request) => BackendEvent::Sheet(Box::new(request)),
                    None => BackendEvent::NeedsYou(NeedsYou::Confirm(id)),
                })
            }
            other => other,
        })
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
        let taken = self
            .log
            .sessions()
            .await?
            .iter()
            .filter(|s| s.as_str().starts_with(&format!("{session}-f")))
            .count();
        let suffix = format!("-f{}", taken + 1);
        // The router's own ids are `s-<n>` and `t-<n>`: a fork's name never meets one of them.
        let child = SessionId::parse(&format!("{session}{suffix}"))
            .map_err(|_| HostFault::NoSuchSession)?;
        let task =
            TaskId::parse(&format!("{parent}{suffix}")).map_err(|_| HostFault::NoSuchSession)?;
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
