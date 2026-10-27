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
//! Every await on the agent has a deadline (`Waits`, with defaults): its handshake, a write to its
//! stdin, and the silence between its lines. One that runs out marks the agent dead, so the turn
//! fails and the host's `close` kills the process. A host may still `cancel`, then `close`, sooner.

use super::cause::Step;
use super::court::Court;
use super::edge::{EdgeFault, EdgeMenu, ToolsEdge, ToolsOffer};
use super::files::Files;
use super::intake::{Intake, Work, intake};
use super::models::{ModelId, Models};
use super::performer::Performer;
use super::reported::Reported;
use super::rpc::{self, Ids};
use super::spawn::{AgentChild, LaunchPlan, SessionMeta, SignIn, Spawn, Spawned};
use super::strikes::Strikes;
use super::taint::TaintSource;
use crate::wire::Wire;
use agent_client_protocol_schema::ProtocolVersion;
use agent_client_protocol_schema::rpc::RequestId;
use agent_client_protocol_schema::v1::{
    AuthMethod, ClientCapabilities, Error, ErrorCode, InitializeResponse, NewSessionResponse,
    PromptResponse, SessionId, StopReason,
};
use bulkhead::Sandbox;
use docket_core::helper::{SignInHow, SignInWay, TerminalLogin};
use docket_core::{AbsPath, CallId, PermissionKind, UserTurn};
use docket_session::{
    BackendEvent, BackendFault, BackendKind, Choice, Choices, ProgramName, ResumePlan, Resumed,
    SessionBackend, StartSession, Taint, TurnEnd,
};
use docket_store::{Waits, within};
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
    pub wire: <X::Spawn as Spawn>::Wire,
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
    pub ids: Ids,
    /// The session's tool edge, alive as long as the connection is.
    pub edge: Option<ToolsEdge>,
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
    /// The host's hold on the request, made the first time it runs and kept so that a run the
    /// router's wait cut short does not stage a second copy.
    pub stage: Option<super::call::StageId>,
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
    pub(super) outbox: VecDeque<String>,
    pub(super) staged: Option<Staged>,
    pub(super) waiters: Vec<(RequestId, serde_json::Value)>,
    pub(super) prompt: Option<RequestId>,
    pub(super) cancelling: bool,
    pub(super) pausing: Option<docket_core::BreakerTrip>,
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
            outbox: VecDeque::new(),
            staged: None,
            waiters: Vec::new(),
            prompt: None,
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

    async fn open(
        &mut self,
        session: prov::SessionId,
        cwd: AbsPath,
        taint: Taint,
    ) -> Result<(), BackendFault> {
        // The agent's writable folder is confined like a terminal's: no link on the way, not the
        // home folder, no place of secrets. The sandbox is given the real directory.
        let workdir = self
            .performer
            .workdir(&cwd, &cwd)
            .map_err(|e| Step::Folder.unavailable(&e))?;
        // The edge is made first: the launcher binds its socket into the sandbox. A host that
        // offers none, or one that cannot make it, runs the agent with its own tools and ours.
        let made = self.tools.as_ref().map(|offer| {
            ToolsEdge::start_with(
                offer,
                &session,
                &self.program,
                self.court.clone(),
                self.menu.clone(),
            )
        });
        self.edge_fault = made.as_ref().and_then(|m| m.as_ref().err().copied());
        let edge = made.and_then(Result::ok);
        let plan = LaunchPlan {
            program: self.program.clone(),
            session: session.clone(),
            cwd: workdir.real().clone(),
            edge: edge.as_ref().map(|e| e.bind().clone()),
        };
        let Spawned {
            wire,
            child,
            meta,
            sign_in,
            model,
        } = self
            .spawn
            .spawn(&plan)
            .await
            .map_err(|e| Step::Spawn.unavailable(&e))?;
        let real_cwd = self
            .performer
            .real(&cwd)
            .map_err(|e| Step::Resolve.unavailable(&e))?;
        self.performer
            .open_scope(&session, cwd.clone(), real_cwd.clone());
        self.live = Some(Live {
            wire,
            child,
            agent: None,
            meta: meta.clone(),
            sign_in: self.sign_in_choice.clone().or(sign_in),
            model,
            models: None,
            ways: Choices(Vec::new()),
            auth_ways: Vec::new(),
            can_log_out: false,
            can_resume: false,
            can_load: false,
            picked_up: false,
            cwd: cwd.clone(),
            real_cwd,
            session: session.clone(),
            tainted_by: (taint == Taint::Tainted).then_some(TaintSource::Resumed),
            reported: Reported::default(),
            ids: Ids::default(),
            edge,
        });
        if taint == Taint::Tainted {
            // A resumed session read something the host cannot see.
            self.performer.note_unseen(&session);
        }
        self.session = Some(session);
        // Signing in may be the person's own browser: that handshake gets the long span.
        let span = match self.live.as_ref().is_some_and(|l| l.sign_in.is_some()) {
            true => self.waits.person,
            false => self.waits.handshake,
        };
        match within(tokio::time::sleep(span), self.handshake(&cwd)).await {
            Ok(done) => done,
            Err(_) => {
                self.dead = true;
                Err(Step::Timeout.unavailable(&"no answer within the handshake span"))
            }
        }
    }

    async fn handshake(&mut self, cwd: &AbsPath) -> Result<(), BackendFault> {
        let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
        let id = live.ids.next();
        let caps = rpc::capabilities(self.performer.capabilities(ClientCapabilities::new()));
        self.send(rpc::initialize(&id, caps)).await?;
        let reply = self.await_reply(&id).await?;
        let init: InitializeResponse =
            serde_json::from_value(reply).map_err(|e| Step::Initialize.unavailable(&e))?;
        if init.protocol_version != ProtocolVersion::V1 {
            return Err(Step::Initialize.unavailable(&"an unsupported protocol version"));
        }
        let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
        live.auth_ways = auth_ways(&init);
        live.can_log_out = init.agent_capabilities.auth.logout.is_some();
        live.can_resume = init
            .agent_capabilities
            .session_capabilities
            .resume
            .is_some();
        live.can_load = init.agent_capabilities.load_session;
        self.sign_in(&init).await?;
        let methods = signable(&init);
        let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
        live.ways = methods.clone();
        if self.resume_session(cwd).await? {
            self.phase = Phase::Idle;
            return Ok(());
        }
        let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
        let id = live.ids.next();
        let servers = live
            .edge
            .iter()
            .map(|e| e.bind().server(e.token()))
            .collect();
        let meta = live.meta.clone();
        self.send(rpc::session_new(&id, cwd, servers, meta)).await?;
        let reply = match self.await_outcome(&id).await? {
            Ok(reply) => reply,
            Err(error) => return Err(refusal_fault(&error, methods)),
        };
        let made: NewSessionResponse =
            serde_json::from_value(reply.clone()).map_err(|e| Step::NewSession.unavailable(&e))?;
        let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
        live.models = Models::of_reply(&reply, &made);
        live.agent = Some(made.session_id);
        self.choose_model().await?;
        self.phase = Phase::Idle;
        Ok(())
    }

    /// Picks up the agent's own session again, when one was asked for and the agent can. False
    /// when there was nothing to resume or the agent would not: a new session is then made.
    async fn resume_session(&mut self, cwd: &AbsPath) -> Result<bool, BackendFault> {
        let Some(old) = self.resume_id.clone() else {
            return Ok(false);
        };
        let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
        // `session/resume` when the agent has it, else `session/load`, which also replays the
        // conversation to us: that replay is the agent's own history, not words of a new turn.
        let (can_resume, can_load) = (live.can_resume, live.can_load);
        if !can_resume && !can_load {
            return Ok(false);
        }
        let id = live.ids.next();
        let servers = live
            .edge
            .iter()
            .map(|e| e.bind().server(e.token()))
            .collect();
        let line = match can_resume {
            true => rpc::session_resume(&id, &old, cwd, servers),
            false => rpc::session_load(&id, &old, cwd, servers),
        };
        self.send(line).await?;
        match self.await_outcome(&id).await? {
            Ok(_) => {
                self.ready.clear();
                let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
                live.agent = Some(old);
                live.picked_up = true;
                Ok(true)
            }
            Err(_) => Ok(false),
        }
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

    /// Asks the agent to forget that it is signed in (`logout`), when it advertised the method.
    /// The agent clears its own login; nothing of it passes through us. A `false` is a signed-in
    /// state the agent would not end, or one it cannot end this way.
    pub async fn log_out(&mut self) -> Result<bool, BackendFault> {
        let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
        if !live.can_log_out || self.phase != Phase::Idle {
            return Ok(false);
        }
        let id = live.ids.next();
        self.send(rpc::logout(&id)).await?;
        Ok(self.await_outcome(&id).await?.is_ok())
    }

    /// Switches to the model `agents.toml` names, when it names one: only one the agent offered,
    /// and the reply is awaited before the first turn. No model named leaves the agent's own.
    async fn choose_model(&mut self) -> Result<(), BackendFault> {
        let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
        let Some(wanted) = live.model.clone() else {
            return Ok(());
        };
        let agent = live.agent.clone().ok_or(BackendFault::NotRunning)?;
        let offer = live.models.clone().ok_or(BackendFault::ModelNotChoosable)?;
        if !offer.offers(&wanted) {
            return Err(BackendFault::ModelNotOffered(offer.choices()));
        }
        if offer.current.as_deref() == Some(wanted.as_str()) {
            return Ok(());
        }
        let id = live.ids.next();
        self.send(offer.switch(&id, &agent, &wanted)).await?;
        match self.await_outcome(&id).await? {
            Ok(_) => {
                let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
                if let Some(models) = live.models.as_mut() {
                    models.current = Some(wanted.as_str().to_owned());
                }
                Ok(())
            }
            Err(_) => Err(BackendFault::ModelRefused),
        }
    }

    /// The models the agent offered for the session, once it is open and when it offers a choice.
    pub fn models(&self) -> Option<&Models> {
        self.live.as_ref().and_then(|l| l.models.as_ref())
    }

    /// The ways of signing in the agent listed when it started, once it has answered `initialize`.
    pub fn sign_in_ways(&self) -> Option<&Choices> {
        self.live.as_ref().map(|l| &l.ways)
    }

    /// Signs in the way `agents.toml` names, when it names one: only a way the agent advertised
    /// as its own to run, and the reply is awaited before `session/new` goes out. The agent reads
    /// its own login; nothing of it passes through us.
    async fn sign_in(&mut self, init: &InitializeResponse) -> Result<(), BackendFault> {
        let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
        let Some(method) = live.sign_in.clone() else {
            return Ok(());
        };
        let offered = init
            .auth_methods
            .iter()
            .any(|m| matches!(m, AuthMethod::Agent(_)) && m.id().0.as_ref() == method.as_str());
        if !offered {
            return Err(BackendFault::SignInUnsupported);
        }
        let id = live.ids.next();
        self.send(rpc::authenticate(&id, &method)).await?;
        // Whatever the agent answers with an error, it did not sign in.
        match self.await_outcome(&id).await? {
            Ok(_) => Ok(()),
            Err(_) => Err(BackendFault::SignInNeeded),
        }
    }

    pub(super) async fn send(&mut self, line: String) -> Result<(), BackendFault> {
        let span = self.waits.prompt;
        let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
        // An agent that stops reading its stdin must not stall the turn for good.
        match within(tokio::time::sleep(span), live.wire.write_line(line)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => {
                self.dead = true;
                Err(Step::Write.unavailable(&e))
            }
            Err(_) => {
                self.dead = true;
                Err(Step::Write.unavailable(&"the write timed out"))
            }
        }
    }

    /// Reads until the reply to `id`. Before a session exists the agent may ask for nothing:
    /// its requests are refused, its notifications dropped.
    async fn await_reply(&mut self, id: &RequestId) -> Result<serde_json::Value, BackendFault> {
        self.await_outcome(id)
            .await?
            .map_err(|error| Step::Reply.unavailable(&error))
    }

    /// Like `await_reply`, but an error the agent answers with is given back, not folded into
    /// "unavailable".
    async fn await_outcome(
        &mut self,
        id: &RequestId,
    ) -> Result<Result<serde_json::Value, serde_json::Value>, BackendFault> {
        loop {
            let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
            let Some(line) = live.wire.read_line().await else {
                self.dead = true;
                return Err(Step::Reply.unavailable(&"the agent closed its output"));
            };
            match intake(&line) {
                Intake::Reply { id: got, outcome } if &got == id => {
                    return Ok(outcome);
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

/// The fault for an error the agent answered `session/new` with: "authentication required"
/// (-32000) is its own fault, everything else is the agent being unavailable.
fn refusal_fault(error: &serde_json::Value, methods: Choices) -> BackendFault {
    match serde_json::from_value::<Error>(error.clone()) {
        Ok(e) if e.code == ErrorCode::AuthRequired && methods.0.is_empty() => {
            BackendFault::SignInNeeded
        }
        Ok(e) if e.code == ErrorCode::AuthRequired => BackendFault::SignInChoose(methods),
        _ => Step::NewSession.unavailable(error),
    }
}

/// Every way of signing in the agent listed, by its own name. A way the client runs in a terminal
/// carries the arguments and variables the agent asked for.
fn auth_ways(init: &InitializeResponse) -> Vec<SignInWay> {
    init.auth_methods
        .iter()
        .map(|m| SignInWay {
            id: m.id().0.to_string(),
            name: m.name().to_owned(),
            how: match m {
                AuthMethod::Terminal(t) => SignInHow::Terminal(TerminalLogin {
                    args: t.args.clone(),
                    env: t.env.clone().into_iter().collect(),
                    run: None,
                }),
                _ => SignInHow::Agent,
            },
        })
        .collect()
}

/// The ways of signing in the agent runs itself, by their names.
fn signable(init: &InitializeResponse) -> Choices {
    Choices(
        init.auth_methods
            .iter()
            .filter(|m| matches!(m, AuthMethod::Agent(_)))
            .map(|m| Choice {
                id: m.id().0.to_string(),
                name: m.name().to_owned(),
            })
            .collect(),
    )
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
            // The edge goes first: a call after this finds no socket, then the process goes.
            drop(live.edge.take());
            live.child.kill();
            live.child.close().await;
        }
    }
}
