//! `AcpBackend`: an external coding agent (Claude Code through its ACP adapter, Gemini CLI, any
//! ACP agent) as a `SessionBackend`. We are the ACP client; the agent is a process behind the
//! `Spawn` seam. Everything it asks of us (`fs/*`, `terminal/*`, `session/request_permission`)
//! is a call the router rules (`court`), never decided here and never by the agent's own options;
//! the file, the command and the answer are performed only after the router allowed the call
//! (`performer`). What the agent only reports is recorded for display and trusted for nothing
//! (`reported`).
//!
//! The two rules a backend keeps (docket-session's contract):
//! - `next_event` is cancel-safe. Everything it holds between awaits is in a field: lines read
//!   but not handled are `ready` and `outbox`, a request that is waiting for its turn is
//!   `staged`. A dropped pull loses nothing and repeats nothing, except that a reply line
//!   interrupted half way through a write may be sent twice (a pipe write of one line).
//! - A call we run for the agent (a read, a write, a command) is announced (`Started`) and runs
//!   on the pull after the one that returned the announcement; a cancel in between answers the
//!   agent "cancelled" and the call never runs.
//!
//! There is no clock here. A deadline for an agent that stops answering is the host's: it calls
//! `cancel`, then `close`, which kills the process.

use super::court::Court;
use super::files::Files;
use super::intake::{Intake, Work, intake};
use super::performer::Performer;
use super::reported::Reported;
use super::rpc::{self, Ids};
use super::spawn::{AgentChild, LaunchPlan, Spawn, Spawned};
use super::strikes::Strikes;
use super::taint::TaintSource;
use crate::wire::Wire;
use agent_client_protocol_schema::ProtocolVersion;
use agent_client_protocol_schema::rpc::RequestId;
use agent_client_protocol_schema::v1::{
    ClientCapabilities, InitializeResponse, NewSessionResponse, PromptResponse, SessionId,
    StopReason,
};
use docket_core::{AbsPath, CallId, PermissionKind, UserTurn};
use docket_session::{
    BackendEvent, BackendFault, BackendKind, ProgramName, ResumePlan, Resumed, SessionBackend,
    StartSession, Taint, TurnEnd,
};
use docket_shell::Sandbox;
use prov::Effect;
use std::collections::VecDeque;

/// The closed set of seams one agent connection is built from.
pub trait Seams {
    /// Starts the agent process.
    type Spawn: Spawn;
    /// The file system `fs/*` reaches.
    type Files: Files + 'static;
    /// The router, as the agent's host calls it.
    type Court: Court;
    /// Confines the commands the agent runs through `terminal/*`.
    type Sandbox: Sandbox + 'static;
}

/// What an `AcpBackend` is built from.
#[allow(missing_debug_implementations)]
pub struct Parts<X: Seams> {
    /// The configured program.
    pub program: ProgramName,
    /// The router session this backend runs (the one the host opened for it).
    pub session: prov::SessionId,
    /// Starts the process.
    pub spawn: X::Spawn,
    /// What performs a call the router allowed: files and the terminal sandbox. The router's
    /// `Perform` for the pseudo-app must reach this very performer.
    pub performer: Performer<X::Files, X::Sandbox>,
    /// The router.
    pub court: X::Court,
}

/// The connection to one running agent.
pub(super) struct Live<X: Seams> {
    pub wire: <X::Spawn as Spawn>::Wire,
    pub child: <X::Spawn as Spawn>::Child,
    pub agent: Option<SessionId>,
    pub cwd: AbsPath,
    pub real_cwd: AbsPath,
    pub session: prov::SessionId,
    pub tainted_by: Option<TaintSource>,
    pub reported: Reported,
    pub ids: Ids,
}

/// A call we run for the agent.
#[derive(Debug, Clone)]
pub(super) struct Called {
    pub call: CallId,
    pub action: docket_core::ActionRef,
    pub effect: Effect,
}

/// A request waiting for its turn to run.
#[derive(Debug, Clone)]
pub(super) struct Staged {
    pub id: RequestId,
    pub work: Work,
    pub call: Option<Called>,
    pub announced: bool,
    pub n: u64,
}

/// Where the connection stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Stopped,
    Idle,
    Answering,
}

/// An external agent as a session backend.
#[allow(missing_debug_implementations)]
pub struct AcpBackend<X: Seams> {
    pub(super) program: ProgramName,
    pub(super) spawn: X::Spawn,
    pub(super) performer: Performer<X::Files, X::Sandbox>,
    pub(super) court: X::Court,
    pub(super) live: Option<Live<X>>,
    pub(super) session: Option<prov::SessionId>,
    pub(super) phase: Phase,
    pub(super) ready: VecDeque<BackendEvent>,
    pub(super) outbox: VecDeque<String>,
    pub(super) staged: Option<Staged>,
    pub(super) waiters: Vec<(RequestId, serde_json::Value)>,
    pub(super) prompt: Option<RequestId>,
    pub(super) cancelling: bool,
    pub(super) pausing: Option<docket_core::BreakerTrip>,
    pub(super) dead: bool,
    pub(super) next_call: u64,
    pub(super) next_ask: u64,
    /// Tool calls the agent reported that bring content in, to be told to the router.
    pub(super) owed: VecDeque<(u64, PermissionKind)>,
    /// Refusals that never reached the router.
    pub(super) strikes: Strikes,
}

impl<X: Seams> AcpBackend<X> {
    /// A backend for the program in `parts`, not yet started.
    pub fn new(parts: Parts<X>) -> Self {
        Self {
            program: parts.program,
            spawn: parts.spawn,
            performer: parts.performer,
            court: parts.court,
            live: None,
            session: Some(parts.session),
            phase: Phase::Stopped,
            ready: VecDeque::new(),
            outbox: VecDeque::new(),
            staged: None,
            waiters: Vec::new(),
            prompt: None,
            cancelling: false,
            pausing: None,
            dead: false,
            next_call: 0,
            next_ask: 0,
            owed: VecDeque::new(),
            strikes: Strikes::default(),
        }
    }

    /// Whether the session is tainted: it served the agent a file or heard its own tools bring
    /// content in. The router holds the taint that rules; this says which thing caused it.
    pub fn tainted(&self) -> bool {
        self.tainted_by().is_some()
    }

    /// The first thing that tainted the session, if anything did.
    pub fn tainted_by(&self) -> Option<&TaintSource> {
        self.live.as_ref().and_then(|l| l.tainted_by.as_ref())
    }

    async fn open(
        &mut self,
        session: prov::SessionId,
        cwd: AbsPath,
        taint: Taint,
    ) -> Result<(), BackendFault> {
        let plan = LaunchPlan {
            program: self.program.clone(),
            session: session.clone(),
            cwd: cwd.clone(),
        };
        let Spawned { wire, child } = self
            .spawn
            .spawn(&plan)
            .await
            .map_err(|_| BackendFault::Unavailable)?;
        let real_cwd = self
            .performer
            .real(&cwd)
            .map_err(|_| BackendFault::Unavailable)?;
        self.performer
            .open_scope(&session, cwd.clone(), real_cwd.clone());
        self.live = Some(Live {
            wire,
            child,
            agent: None,
            cwd: cwd.clone(),
            real_cwd,
            session: session.clone(),
            tainted_by: (taint == Taint::Tainted).then_some(TaintSource::Resumed),
            reported: Reported::default(),
            ids: Ids::default(),
        });
        if taint == Taint::Tainted {
            // A resumed session read something the host cannot see.
            self.performer.note_unseen(&session);
        }
        self.session = Some(session);
        self.handshake(&cwd).await
    }

    async fn handshake(&mut self, cwd: &AbsPath) -> Result<(), BackendFault> {
        let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
        let id = live.ids.next();
        let caps = rpc::capabilities(self.performer.capabilities(ClientCapabilities::new()));
        self.send(rpc::initialize(&id, caps)).await?;
        let reply = self.await_reply(&id).await?;
        let init: InitializeResponse =
            serde_json::from_value(reply).map_err(|_| BackendFault::Unavailable)?;
        if init.protocol_version != ProtocolVersion::V1 {
            return Err(BackendFault::Unavailable);
        }
        let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
        let id = live.ids.next();
        self.send(rpc::session_new(&id, cwd)).await?;
        let reply = self.await_reply(&id).await?;
        let made: NewSessionResponse =
            serde_json::from_value(reply).map_err(|_| BackendFault::Unavailable)?;
        let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
        live.agent = Some(made.session_id);
        self.phase = Phase::Idle;
        Ok(())
    }

    pub(super) async fn send(&mut self, line: String) -> Result<(), BackendFault> {
        let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
        live.wire.write_line(line).await.map_err(|_| {
            self.dead = true;
            BackendFault::Unavailable
        })
    }

    /// Reads until the reply to `id`. Before a session exists the agent may ask for nothing:
    /// its requests are refused, its notifications dropped.
    async fn await_reply(&mut self, id: &RequestId) -> Result<serde_json::Value, BackendFault> {
        loop {
            let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
            let Some(line) = live.wire.read_line().await else {
                self.dead = true;
                return Err(BackendFault::Unavailable);
            };
            match intake(&line) {
                Intake::Reply { id: got, outcome } if &got == id => {
                    return outcome.map_err(|_| BackendFault::Unavailable);
                }
                Intake::Request { id: theirs, .. } => {
                    let refusal = super::serve::refusal(&theirs, "not now");
                    self.send(refusal).await?;
                }
                _ => {}
            }
        }
    }
}

fn stop(reason: StopReason) -> TurnEnd {
    match reason {
        StopReason::EndTurn => TurnEnd::Done,
        StopReason::Refusal => TurnEnd::Refused,
        StopReason::Cancelled => TurnEnd::Cancelled,
        _ => TurnEnd::Failed,
    }
}

impl<X: Seams> AcpBackend<X> {
    /// A turn is over: the end event, after whatever the agent left open.
    pub(super) fn turn_over(&mut self, outcome: Result<serde_json::Value, serde_json::Value>) {
        let end = match (&self.pausing, outcome) {
            (Some(trip), _) => TurnEnd::Paused(*trip),
            (None, Ok(value)) => match serde_json::from_value::<PromptResponse>(value) {
                Ok(_) if self.cancelling => TurnEnd::Cancelled,
                Ok(done) => stop(done.stop_reason),
                Err(_) => TurnEnd::Failed,
            },
            (None, Err(_)) => TurnEnd::Failed,
        };
        self.finish_turn(end);
    }

    pub(super) fn finish_turn(&mut self, end: TurnEnd) {
        if let Some(live) = self.live.as_mut() {
            self.ready.extend(live.reported.close_turn());
        }
        self.ready.push_back(BackendEvent::TurnEnd(end));
        self.prompt = None;
        self.cancelling = false;
        self.pausing = None;
        self.staged = None;
        self.phase = Phase::Idle;
    }
}

impl<X: Seams> SessionBackend for AcpBackend<X> {
    fn kind(&self) -> BackendKind {
        BackendKind::Acp(self.program.clone())
    }

    async fn start(&mut self, open: StartSession) -> Result<(), BackendFault> {
        let cwd = open
            .opening
            .cwd
            .as_ref()
            .and_then(|w| AbsPath::parse(w.as_str()).ok())
            .ok_or(BackendFault::Unavailable)?;
        self.open(open.session, cwd, Taint::Clean).await
    }

    async fn resume(&mut self, plan: &ResumePlan) -> Result<Resumed, BackendFault> {
        // The agent's own session id is not kept in the log, so a resume is always a new agent
        // session; the person is told (`Reseeded`).
        let cwd = plan
            .opening
            .cwd
            .as_ref()
            .and_then(|w| AbsPath::parse(w.as_str()).ok())
            .ok_or(BackendFault::Unavailable)?;
        let session = self.session.clone().ok_or(BackendFault::NotRunning)?;
        self.open(session, cwd, plan.taint).await?;
        Ok(Resumed::Reseeded)
    }

    async fn turn(&mut self, turn: UserTurn) -> Result<(), BackendFault> {
        match self.phase {
            Phase::Stopped => return Err(BackendFault::NotRunning),
            Phase::Answering => return Err(BackendFault::Busy),
            Phase::Idle if self.dead => return Err(BackendFault::Unavailable),
            Phase::Idle => {}
        }
        let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
        let agent = live.agent.clone().ok_or(BackendFault::NotRunning)?;
        self.strikes.new_turn();
        let id = live.ids.next();
        self.send(rpc::prompt(&id, &agent, &turn.text)).await?;
        self.prompt = Some(id);
        self.phase = Phase::Answering;
        Ok(())
    }

    async fn next_event(&mut self) -> Option<BackendEvent> {
        self.pull().await
    }

    async fn cancel(&mut self) {
        self.stop_turn().await;
    }

    async fn close(&mut self) {
        self.phase = Phase::Stopped;
        self.ready.clear();
        self.outbox.clear();
        self.staged = None;
        self.waiters.clear();
        if let Some(mut live) = self.live.take() {
            self.performer.close_scope(&live.session);
            live.child.kill();
            live.child.close().await;
        }
    }
}
