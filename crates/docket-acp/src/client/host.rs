//! `AgentHost`: the `SessionHost` of one external agent. A sibling of the native host, for the
//! same edges: it opens the agent's session at the router (which writes the session's log, as it
//! does for every session), records the person's turn there (the router derives the task policy
//! from the person's words, as for a launcher), and runs the turn on an `AcpBackend`.
//!
//! **Sheets.** The router asks the person through its confirmer, like any call: sill's sheet, by
//! default, and the host sees nothing of it. In the development fallback (`Fallback::Terminal`,
//! `docket-agent --tty`) the host tells the router that it shows this session's sheets itself,
//! and each one comes out of `next_event` as `BackendEvent::Sheet` and goes back through
//! `answer_sheet`, as an editor's do. Without the flag the host asks for no such route and holds
//! no desk, so a sheet can only go to the desktop.
//!
//! One host runs one agent session at a time; resuming or forking one is not served (ACP v1 has
//! no fork, and a resumed agent session is always a new one, which the router restores itself).

use super::backend::{AcpBackend, Seams};
use super::cause::Step;
use super::court::{Court, OpenAgent};
use super::running::RunningTurn;
use docket_core::{
    ConfirmId, HandTo, Rewind, SheetSurface, TurnEnd as Ended, TurnId, UserTurn, WatchedAgent,
};
use docket_session::{
    BackendEvent, BackendFault, BackendKind, EndCause, HostFault, Opening, ResumePlan,
    SessionBackend, SessionExport, SessionHost, SheetChoice, SheetDesk, StartSession, TurnEnd,
};
use futures_util::future::{Either, select};
use prov::SessionId;
use std::future::Future;
use std::pin::{Pin, pin};

/// Where the person answers the router's sheets for this agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallback {
    /// The desktop's sheet, through the router's confirmer: the normal path.
    Off,
    /// This host, on the terminal: a development fallback, never the default.
    Terminal,
}

impl Fallback {
    fn surface(self) -> SheetSurface {
        match self {
            Fallback::Off => SheetSurface::Desktop,
            Fallback::Terminal => SheetSurface::Host,
        }
    }
}

type Recording = Pin<Box<dyn Future<Output = Result<TurnId, super::court::CourtFault>> + Send>>;

/// A turn the router is recording and the agent has not yet been given.
struct Opening2 {
    turn: UserTurn,
    recording: Recording,
}

/// The host of one agent session.
pub struct AgentHost<X: Seams, D: SheetDesk> {
    backend: AcpBackend<X>,
    court: X::Court,
    desk: D,
    fallback: Fallback,
    rewind: Rewind,
    helper: Option<prov::HelperId>,
    session: Option<SessionId>,
    pending: Option<Opening2>,
    /// The turn the agent is working on; the router hears when it ends, whichever way.
    running: Option<RunningTurn<X::Court>>,
}

impl<X: Seams, D: SheetDesk> std::fmt::Debug for AgentHost<X, D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AgentHost")
    }
}

impl<X: Seams, D: SheetDesk> AgentHost<X, D> {
    /// A host over `backend` and the `court` it calls the router through (a clone of the
    /// backend's), with `desk` holding the sheets the router puts to this host when `fallback`
    /// asks for them.
    pub fn new(backend: AcpBackend<X>, court: X::Court, desk: D, fallback: Fallback) -> Self {
        Self {
            backend,
            court,
            desk,
            fallback,
            rewind: Rewind::default(),
            helper: None,
            session: None,
            pending: None,
            running: None,
        }
    }

    /// Says who keeps the history of the agent's file changes (its `checkpoints` in
    /// `agents.toml`); the router is told when the session opens. Without it the default over-saves
    /// rather than under-saves.
    pub fn with_rewind(mut self, rewind: Rewind) -> Self {
        self.rewind = rewind;
        self
    }

    /// Says that the agent is one helper's: the router keeps its grants apart from every other
    /// helper's, even one that runs the same program.
    pub fn for_helper(mut self, helper: prov::HelperId) -> Self {
        self.helper = Some(helper);
        self
    }

    /// Ends the agent's process after a start that failed (the agent wanted signing in, say): the
    /// session never opened, so [`SessionHost::close`] has nothing to close, but the process is
    /// running and must not be left so.
    pub async fn abandon(&mut self) {
        self.backend.close().await;
    }

    /// Asks the agent to sign out (`logout`), when it advertised the method. `Ok(false)` when it
    /// did not, or would not. The agent clears its own login.
    pub async fn log_out(&mut self) -> Result<bool, BackendFault> {
        self.backend.log_out().await
    }

    /// Leaves the open session at the router as a watch session that a terminal marks, and ends
    /// the agent's process: the conversation goes on in the app's own terminal program. `agent` is
    /// the app and its own id for the conversation. The router refuses while a turn is running.
    pub async fn hand_to_terminal(
        &mut self,
        session: &SessionId,
        agent: WatchedAgent,
    ) -> Result<(), HostFault> {
        self.mine(session)?;
        self.court
            .hand(session, HandTo::Terminal, Some(agent))
            .await
            .map_err(|_| BackendFault::Unavailable)?;
        self.pending = None;
        self.running = None;
        self.backend.close().await;
        self.session = None;
        Ok(())
    }

    /// Takes a session back that the router holds as a watch session, and starts the agent on it
    /// (`opening` says where; the backend was made to resume the agent's own session). Answers
    /// the app and its own id for the conversation. A start that fails leaves the session a watch
    /// session, as it was.
    pub async fn take_back(
        &mut self,
        session: SessionId,
        opening: Opening,
    ) -> Result<WatchedAgent, HostFault> {
        if self.session.is_some() {
            return Err(BackendFault::Busy.into());
        }
        let agent = self
            .court
            .hand(&session, HandTo::Desktop, None)
            .await
            .ok()
            .flatten()
            .ok_or(BackendFault::Unavailable)?;
        let started = self
            .backend
            .start(StartSession {
                session: session.clone(),
                opening,
            })
            .await;
        if let Err(fault) = started {
            self.backend.close().await;
            let _ = self
                .court
                .hand(&session, HandTo::Terminal, Some(agent))
                .await;
            return Err(fault.into());
        }
        self.session = Some(session);
        Ok(agent)
    }

    /// The backend, for what only it knows (the taint's cause).
    pub fn backend(&self) -> &AcpBackend<X> {
        &self.backend
    }

    fn mine(&self, session: &SessionId) -> Result<(), HostFault> {
        (self.session.as_ref() == Some(session))
            .then_some(())
            .ok_or(HostFault::NoSuchSession)
    }

    fn is_terminal(&self) -> bool {
        self.fallback == Fallback::Terminal
    }
}

/// Tells the router the turn ended, if `event` is the end of one (or the backend has nothing
/// more to say, which ends it too). Any other event leaves the turn running.
async fn settle<C: Court>(running: &mut Option<RunningTurn<C>>, event: &Option<BackendEvent>) {
    let how = match event {
        Some(BackendEvent::TurnEnd(end)) => Ended::from(*end),
        None => Ended::Failed,
        Some(_) => return,
    };
    if let Some(turn) = running.take() {
        turn.end(how).await;
    }
}

fn program_of(opening: &Opening) -> Option<docket_session::ProgramName> {
    match &opening.backend {
        BackendKind::Acp(program) => Some(program.clone()),
        BackendKind::Native | BackendKind::Fake | BackendKind::Watch(_) => None,
    }
}

impl<X: Seams, D: SheetDesk> SessionHost for AgentHost<X, D> {
    async fn open(&mut self, opening: Opening) -> Result<SessionId, HostFault> {
        if self.session.is_some() {
            return Err(BackendFault::Busy.into());
        }
        let program = program_of(&opening)
            .ok_or_else(|| Step::Opening.unavailable(&"it is not for an agent program"))?;
        let cwd = opening
            .cwd
            .clone()
            .ok_or_else(|| Step::Opening.unavailable(&"it names no folder"))?;
        let session = self
            .court
            .open(OpenAgent {
                program,
                cwd,
                sheets: self.fallback.surface(),
                space: opening.space.clone(),
                label: opening.label.clone(),
                rewind: self.rewind,
                helper: self.helper.clone(),
            })
            .await
            .map_err(|e| Step::Router.unavailable(&e))?;
        let started = self
            .backend
            .start(StartSession {
                session: session.clone(),
                opening,
            })
            .await;
        if let Err(fault) = started {
            self.court.close(&session).await;
            return Err(fault.into());
        }
        self.session = Some(session.clone());
        Ok(session)
    }

    async fn resume(&mut self, _session: &SessionId) -> Result<ResumePlan, HostFault> {
        Err(HostFault::NoSuchSession)
    }

    async fn turn(&mut self, session: &SessionId, turn: UserTurn) -> Result<(), HostFault> {
        self.mine(session)?;
        if self.pending.is_some() {
            return Err(BackendFault::Busy.into());
        }
        // The router records what the person said, and may ask the person about it (a turn that
        // widens the task): that wait happens in `next_event`, where a sheet can be shown.
        let (mut court, id, text) = (self.court.clone(), session.clone(), turn.text.clone());
        let recording: Recording = Box::pin(async move { court.turn(&id, &text).await });
        self.pending = Some(Opening2 { turn, recording });
        Ok(())
    }

    async fn next_event(&mut self, session: &SessionId) -> Result<Option<BackendEvent>, HostFault> {
        self.mine(session)?;
        loop {
            if let Some(pending) = self.pending.as_mut() {
                let sheet = pin!(self.desk.next_sheet(session));
                let recorded = match (
                    self.fallback,
                    select(pending.recording.as_mut(), sheet).await,
                ) {
                    (_, Either::Left((recorded, _))) => recorded,
                    (Fallback::Terminal, Either::Right((Some(request), _))) => {
                        return Ok(Some(BackendEvent::Sheet(Box::new(request))));
                    }
                    // No desk is asked for: nothing can be on it.
                    (_, Either::Right(_)) => continue,
                };
                let Some(Opening2 { turn, .. }) = self.pending.take() else {
                    continue;
                };
                let Ok(recorded) = recorded else {
                    return Ok(Some(BackendEvent::TurnEnd(TurnEnd::Failed)));
                };
                // From here the router counts the turn as running; the guard says when it is not.
                let guard = RunningTurn::begin(self.court.clone(), session.clone(), recorded);
                self.running = Some(guard);
                if let Err(fault) = self.backend.turn(turn).await {
                    if let Some(turn) = self.running.take() {
                        turn.end(Ended::Failed).await;
                    }
                    return Err(fault.into());
                }
                continue;
            }
            if !self.is_terminal() {
                let event = self.backend.next_event().await;
                settle(&mut self.running, &event).await;
                return Ok(event);
            }
            let event = pin!(self.backend.next_event());
            let sheet = pin!(self.desk.next_sheet(session));
            return Ok(match select(event, sheet).await {
                Either::Left((event, _)) => {
                    settle(&mut self.running, &event).await;
                    event
                }
                Either::Right((Some(request), _)) => Some(BackendEvent::Sheet(Box::new(request))),
                // A desk with nothing to hand out ends the race: the backend's event is what is
                // left, and the next pull asks for it again.
                Either::Right((None, _)) => continue,
            });
        }
    }

    async fn answer_sheet(
        &mut self,
        session: &SessionId,
        id: &ConfirmId,
        choice: SheetChoice,
    ) -> Result<(), HostFault> {
        self.mine(session)?;
        self.desk.answer(id, choice)?;
        Ok(())
    }

    async fn cancel(&mut self, session: &SessionId) -> Result<(), HostFault> {
        self.mine(session)?;
        self.backend.cancel().await;
        Ok(())
    }

    async fn close(&mut self, session: &SessionId, _cause: EndCause) -> Result<(), HostFault> {
        self.mine(session)?;
        self.pending = None;
        if let Some(turn) = self.running.take() {
            turn.end(Ended::Cancelled).await;
        }
        self.backend.close().await;
        self.court.close(session).await;
        self.session = None;
        Ok(())
    }

    async fn fork(
        &mut self,
        _session: &SessionId,
        _at: docket_session::Seq,
    ) -> Result<SessionId, HostFault> {
        Err(HostFault::NoSuchSession)
    }

    fn export(
        &self,
        _session: &SessionId,
    ) -> impl Future<Output = Result<SessionExport, HostFault>> + Send {
        std::future::ready(Err(HostFault::NoSuchSession))
    }
}
