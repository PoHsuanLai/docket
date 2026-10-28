//! `AcpBackend`: an external coding agent (Claude Code through its ACP adapter, Gemini CLI, any
//! ACP agent) as a `SessionBackend`. We are the ACP client, on the SDK's client role (`link`); the
//! agent is a process behind the `Spawn` seam. Everything it asks of us (`fs/*`, `terminal/*`,
//! `session/request_permission`) is a call the router rules (`court`), never decided here and
//! never by the agent's own options; the file, the command and the answer are performed only after
//! the router allowed the call (`performer`). What the agent only reports is recorded for display
//! and trusted for nothing (`reported`).
//!
//! The two rules a backend keeps (docket-session's contract):
//! - `next_event` is cancel-safe. Everything it holds between awaits is in a field: events heard
//!   but not handed on are `ready`, messages the SDK has taken but we have not are in the link's
//!   queue, a request that is waiting for its turn is `staged`. A dropped pull loses nothing and
//!   repeats nothing; a reply is sent whole, in one step.
//! - A call we run for the agent (a read, a write, a command) is announced (`Started`) and runs
//!   on the pull after the one that returned the announcement; a cancel in between answers the
//!   agent "cancelled" and the call never runs.
//!
//! Every await on the agent has a deadline (`Waits`, with defaults): its handshake, and the silence
//! between its messages. One that runs out marks the agent dead, so the turn fails and the host's
//! `close` kills the process. A host may still `cancel`, then `close`, sooner. A write the agent
//! never reads ends the connection at the line sink's span (`capped_lines`).

use super::edge::{EdgeFault, EdgeMenu, ToolsEdge, ToolsOffer};
use super::files::Files;
use super::link::{Gone, Link, Reply, Work};
use super::models::{ModelId, Models};
use super::performer::Performer;
use super::spawn::{AgentChild, SessionMeta, SignIn, Spawn};
use acp_core::cause::Step;
use acp_core::court::Court;
use acp_core::reported::Reported;
use acp_core::strikes::Strikes;
use acp_core::taint::TaintSource;
use agent_client_protocol::schema::v1::{
    ContentBlock, Error, PromptRequest, PromptResponse, SessionId, StopReason,
    WaitForTerminalExitRequest,
};
use bulkhead::AbsPath;
use bulkhead::Sandbox;
use companion_model::helper::SignInWay;
use docket_core::UserTurn;
use docket_permissions_core::PermissionKind;
use docket_permissions_core::ProgramName;
use docket_session::{
    BackendEvent, BackendFault, BackendKind, Choices, ResumePlan, Resumed, SessionBackend,
    StartSession, Taint, TurnEnd,
};
use docket_store::Waits;
use docket_vocab::CallId;
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
    /// The tool edge to offer the agent, when the host has one to give.
    pub tools: Option<ToolsOffer>,
}

/// The connection to one running agent.
pub(super) struct Live<X: Seams> {
    pub link: Link,
    pub child: <X::Spawn as Spawn>::Child,
    pub agent: Option<SessionId>,
    /// The launcher's `_meta` for `session/new`, kept for the handshake.
    pub meta: Option<SessionMeta>,
    /// The way to sign in before `session/new`, kept for the handshake.
    pub sign_in: Option<SignIn>,
    /// The model to switch to after `session/new`, kept for the handshake.
    pub model: Option<ModelId>,
    /// The models the agent offered for this session.
    pub models: Option<Models>,
    /// The ways of signing in the agent listed at `initialize`.
    pub ways: Choices,
    /// Every way it listed, the ones that need a terminal too, with what each needs.
    pub auth_ways: Vec<SignInWay>,
    /// Whether it advertised `logout`.
    pub can_log_out: bool,
    /// Whether it advertised `session/resume`.
    pub can_resume: bool,
    /// Whether it advertised `session/load`, which replays the conversation to us as it picks
    /// it up.
    pub can_load: bool,
    /// Whether this start picked a session of its own up again instead of making a new one.
    pub picked_up: bool,
    pub cwd: AbsPath,
    pub real_cwd: AbsPath,
    pub session: prov::SessionId,
    pub tainted_by: Option<TaintSource>,
    pub reported: Reported,
    /// The session's tool edge, alive as long as the connection is.
    pub edge: Option<ToolsEdge>,
}

/// A call we run for the agent.
#[derive(Debug, Clone)]
pub(super) struct Called {
    pub call: CallId,
    pub action: docket_vocab::ActionRef,
    pub effect: Effect,
}

/// A request waiting for its turn to run.
pub(super) struct Staged {
    pub work: Work,
    /// How to answer it.
    pub reply: Reply,
    pub call: Option<Called>,
    /// The host's hold on the request, made the first time it runs and kept so that a run the
    /// router's wait cut short does not stage a second copy.
    pub stage: Option<acp_core::call::StageId>,
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
    pub(super) tools: Option<ToolsOffer>,
    pub(super) menu: EdgeMenu,
    pub(super) live: Option<Live<X>>,
    pub(super) session: Option<prov::SessionId>,
    pub(super) phase: Phase,
    pub(super) ready: VecDeque<BackendEvent>,
    pub(super) staged: Option<Staged>,
    /// `terminal/wait_for_exit` requests whose command is still running.
    pub(super) waiters: Vec<(Reply, WaitForTerminalExitRequest)>,
    pub(super) cancelling: bool,
    pub(super) pausing: Option<docket_permissions_core::BreakerTrip>,
    pub(super) dead: bool,
    /// Why the tool edge could not be made at the last start, when it was offered and failed.
    pub(super) edge_fault: Option<EdgeFault>,
    pub(super) next_call: u64,
    pub(super) next_ask: u64,
    /// Tool calls the agent reported that bring content in, to be told to the router.
    pub(super) owed: VecDeque<(u64, PermissionKind)>,
    /// Refusals that never reached the router.
    pub(super) strikes: Strikes,
    /// How long the agent may take for each kind of wait.
    pub(super) waits: Waits,
    /// A way of signing in the person chose for this start, over the one `agents.toml` names.
    pub(super) sign_in_choice: Option<SignIn>,
    /// The agent's own session to pick up again, when it can: a helper's process that was
    /// restarted goes on with the session it had.
    pub(super) resume_id: Option<SessionId>,
}

impl<X: Seams> AcpBackend<X> {
    /// A backend for the program in `parts`, not yet started.
    pub fn new(parts: Parts<X>) -> Self {
        Self {
            program: parts.program,
            spawn: parts.spawn,
            performer: parts.performer,
            court: parts.court,
            tools: parts.tools,
            menu: EdgeMenu::Everything,
            live: None,
            session: Some(parts.session),
            phase: Phase::Stopped,
            ready: VecDeque::new(),
            staged: None,
            waiters: Vec::new(),
            cancelling: false,
            pausing: None,
            dead: false,
            edge_fault: None,
            next_call: 0,
            next_ask: 0,
            owed: VecDeque::new(),
            strikes: Strikes::default(),
            waits: Waits::default(),
            sign_in_choice: None,
            resume_id: None,
        }
    }

    /// The same backend, signing in with `way` before it opens its session: the person chose it
    /// on the helper's card, from the ways the agent listed. Only a way the agent runs itself;
    /// one that needs a terminal is never passed to `authenticate`.
    pub fn with_sign_in(self, way: SignIn) -> Self {
        Self {
            sign_in_choice: Some(way),
            ..self
        }
    }

    /// The same backend, its tool edge offering only what `menu` allows: a helper's agent gets
    /// its card's menu, not the assistant's.
    pub fn with_menu(self, menu: EdgeMenu) -> Self {
        Self { menu, ..self }
    }

    /// The same backend, picking up the agent's own session `session` again when the agent
    /// advertises `session/resume`, and opening a new one when it does not or will not.
    pub fn resuming(self, session: &str) -> Self {
        Self {
            resume_id: Some(SessionId::new(session)),
            ..self
        }
    }

    /// The same backend, waiting on its agent as `waits` says.
    pub fn with_waits(self, waits: Waits) -> Self {
        Self { waits, ..self }
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

    /// Why the agent runs without the desktop's tools, when the host offered them and the edge
    /// could not be made: the person should be told, not left to wonder.
    pub fn edge_fault(&self) -> Option<EdgeFault> {
        self.edge_fault
    }

    /// Whether this start picked the agent's own session up again, as asked by
    /// [`AcpBackend::resuming`], instead of making a new one.
    pub fn picked_up(&self) -> bool {
        self.live.as_ref().is_some_and(|live| live.picked_up)
    }

    /// Whether the agent can pick a conversation of its own up again (`session/resume` or
    /// `session/load`), once it has said so at `initialize`.
    pub fn picks_up_again(&self) -> bool {
        self.live
            .as_ref()
            .is_some_and(|live| live.can_resume || live.can_load)
    }

    /// Every way of signing in the agent listed when it started, the ones that need a terminal
    /// too, by the agent's own names. Empty before it has answered `initialize`.
    pub fn auth_ways(&self) -> &[SignInWay] {
        match self.live.as_ref() {
            Some(live) => live.auth_ways.as_slice(),
            None => &[],
        }
    }

    /// Whether the agent advertised `logout`.
    pub fn can_log_out(&self) -> bool {
        self.live.as_ref().is_some_and(|l| l.can_log_out)
    }

    /// The agent's own id for its session, once it is open: what a restarted process resumes.
    pub fn agent_session(&self) -> Option<String> {
        let live = self.live.as_ref()?;
        live.agent.as_ref().map(|id| id.0.to_string())
    }

    /// The models the agent offered for the session, once it is open and when it offers a choice.
    pub fn models(&self) -> Option<&Models> {
        self.live.as_ref().and_then(|l| l.models.as_ref())
    }

    /// The ways of signing in the agent listed when it started, once it has answered `initialize`.
    pub fn sign_in_ways(&self) -> Option<&Choices> {
        self.live.as_ref().map(|l| &l.ways)
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
    pub(super) fn turn_over(&mut self, outcome: Result<PromptResponse, Error>) {
        let end = match (&self.pausing, outcome) {
            (Some(trip), _) => TurnEnd::Paused(*trip),
            (None, Ok(_)) if self.cancelling => TurnEnd::Cancelled,
            (None, Ok(done)) => stop(done.stop_reason),
            (None, Err(_)) => TurnEnd::Failed,
        };
        self.finish_turn(end);
    }

    pub(super) fn finish_turn(&mut self, end: TurnEnd) {
        if let Some(live) = self.live.as_mut() {
            self.ready.extend(live.reported.close_turn());
        }
        self.ready.push_back(BackendEvent::TurnEnd(end));
        self.cancelling = false;
        self.pausing = None;
        self.drop_staged();
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
        let live = self.live.as_ref().ok_or(BackendFault::NotRunning)?;
        let agent = live.agent.clone().ok_or(BackendFault::NotRunning)?;
        self.strikes.new_turn();
        let words = vec![ContentBlock::from(turn.text)];
        if live.link.prompt(PromptRequest::new(agent, words)).is_err() {
            self.dead = true;
            return Err(Step::Write.unavailable(&Gone));
        }
        self.phase = Phase::Answering;
        Ok(())
    }

    async fn next_event(&mut self) -> Option<BackendEvent> {
        self.pull().await
    }

    async fn cancel(&mut self) {
        self.stop_turn();
    }

    async fn close(&mut self) {
        self.phase = Phase::Stopped;
        self.ready.clear();
        self.staged = None;
        self.waiters.clear();
        if let Some(mut live) = self.live.take() {
            self.performer.close_scope(&live.session);
            // The edge goes first: a call after this finds no socket, then the process goes, and
            // the connection with `live`.
            drop(live.edge.take());
            live.child.kill();
            live.child.close().await;
        }
    }
}
