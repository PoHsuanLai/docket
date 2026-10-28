//! How a connection to an agent opens: the process, the SDK link, `initialize`, the sign-in the
//! person chose, `session/new` (or the agent's own session picked up again) and the model. Every
//! answer is awaited before the next request goes out, and a message the agent sends meanwhile
//! is turned away (see `Link::answer`).

use super::backend::{AcpBackend, Live, Phase, Seams};
use super::edge::ToolsEdge;
use super::link::{Gone, Link};
use super::models::Models;
use super::spawn::{AgentChild, LaunchPlan, Spawn, Spawned};
use acp_core::cause::Step;
use acp_core::reported::Reported;
use acp_core::taint::TaintSource;
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    AGENT_METHOD_NAMES, AuthCapabilities, AuthMethod, AuthenticateRequest, ClientCapabilities,
    Error, ErrorCode, FileSystemCapabilities, Implementation, InitializeRequest,
    InitializeResponse, LoadSessionRequest, LogoutRequest, NewSessionRequest, NewSessionResponse,
    ResumeSessionRequest,
};
use agent_client_protocol::{SentRequest, UntypedMessage};
use bulkhead::AbsPath;
use docket_core::helper::{SignInHow, SignInWay, TerminalLogin};
use docket_session::{BackendFault, Choice, Choices, Taint};
use std::path::PathBuf;

/// What we offer the agent: file reads and writes through us, a terminal when `terminal` says the
/// sandbox is there, and a terminal for the agent's own sign-in (the shell opens it, running the
/// agent's command in the helper's sandbox). Nothing else (no elicitation).
fn capabilities(terminal: ClientCapabilities) -> ClientCapabilities {
    terminal
        .fs(FileSystemCapabilities::new()
            .read_text_file(true)
            .write_text_file(true))
        .auth(AuthCapabilities::new().terminal(true))
}

impl<X: Seams> AcpBackend<X> {
    pub(super) async fn open(
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
            transport,
            mut child,
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
        let link = match Link::connect(transport).await {
            Ok(link) => link,
            Err(gone) => {
                child.kill();
                child.close().await;
                return Err(Step::Spawn.unavailable(&gone));
            }
        };
        self.performer
            .open_scope(&session, cwd.clone(), real_cwd.clone());
        self.live = Some(Live {
            link,
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
        match tokio::time::timeout(span, self.handshake(&cwd)).await {
            Ok(done) => done,
            Err(_) => {
                self.dead = true;
                Err(Step::Timeout.unavailable(&"no answer within the handshake span"))
            }
        }
    }

    /// Waits for the agent's answer to `sent`: its result, or the error it answered with. An
    /// agent that has closed the connection is dead.
    async fn answered<T: Send>(
        &mut self,
        sent: SentRequest<T>,
    ) -> Result<Result<T, Error>, BackendFault> {
        let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
        match live.link.answer(sent).await {
            Ok(answer) => Ok(answer),
            Err(Gone) => {
                self.dead = true;
                Err(Step::Reply.unavailable(&Gone))
            }
        }
    }

    fn link(&self) -> Result<&Link, BackendFault> {
        self.live
            .as_ref()
            .map(|live| &live.link)
            .ok_or(BackendFault::NotRunning)
    }

    async fn handshake(&mut self, cwd: &AbsPath) -> Result<(), BackendFault> {
        let caps = capabilities(self.performer.capabilities(ClientCapabilities::new()));
        let initialize = InitializeRequest::new(ProtocolVersion::V1)
            .client_capabilities(caps)
            .client_info(Implementation::new("docket", env!("CARGO_PKG_VERSION")));
        let sent = self.link()?.ask(initialize);
        // The SDK refuses an answer that does not pick protocol version 1.
        let init = self
            .answered(sent)
            .await?
            .map_err(|e| Step::Initialize.unavailable(&e))?;
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
        let live = self.live.as_ref().ok_or(BackendFault::NotRunning)?;
        let servers = live
            .edge
            .iter()
            .map(|e| e.bind().server(e.token()))
            .collect();
        let request = NewSessionRequest::new(PathBuf::from(cwd.as_str()))
            .mcp_servers(servers)
            .meta(live.meta.clone());
        // Sent untyped to read the whole answer: some agents still list their models in a field
        // the schema no longer has (`Models::of_reply`).
        let message = UntypedMessage::new(AGENT_METHOD_NAMES.session_new, &request)
            .map_err(|e| Step::NewSession.unavailable(&e))?;
        let sent = live.link.ask(message);
        let reply = match self.answered(sent).await? {
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
        let live = self.live.as_ref().ok_or(BackendFault::NotRunning)?;
        // `session/resume` when the agent has it, else `session/load`, which also replays the
        // conversation to us: that replay is the agent's own history, not words of a new turn.
        let (can_resume, can_load) = (live.can_resume, live.can_load);
        if !can_resume && !can_load {
            return Ok(false);
        }
        let servers = live
            .edge
            .iter()
            .map(|e| e.bind().server(e.token()))
            .collect();
        let path = PathBuf::from(cwd.as_str());
        let picked = match can_resume {
            true => {
                let request = ResumeSessionRequest::new(old.clone(), path).mcp_servers(servers);
                let sent = live.link.ask(request);
                self.answered(sent).await?.map(|_| ())
            }
            false => {
                let request = LoadSessionRequest::new(old.clone(), path).mcp_servers(servers);
                let sent = live.link.ask(request);
                self.answered(sent).await?.map(|_| ())
            }
        };
        match picked {
            Ok(()) => {
                self.ready.clear();
                let live = self.live.as_mut().ok_or(BackendFault::NotRunning)?;
                live.agent = Some(old);
                live.picked_up = true;
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }

    /// Asks the agent to forget that it is signed in (`logout`), when it advertised the method.
    /// The agent clears its own login; nothing of it passes through us. A `false` is a signed-in
    /// state the agent would not end, or one it cannot end this way.
    pub async fn log_out(&mut self) -> Result<bool, BackendFault> {
        let live = self.live.as_ref().ok_or(BackendFault::NotRunning)?;
        if !live.can_log_out || self.phase != Phase::Idle {
            return Ok(false);
        }
        let sent = live.link.ask(LogoutRequest::new());
        Ok(self.answered(sent).await?.is_ok())
    }

    /// Switches to the model `agents.toml` names, when it names one: only one the agent offered,
    /// and the reply is awaited before the first turn. No model named leaves the agent's own.
    async fn choose_model(&mut self) -> Result<(), BackendFault> {
        let live = self.live.as_ref().ok_or(BackendFault::NotRunning)?;
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
        let message = offer
            .switch(&agent, &wanted)
            .map_err(|_| BackendFault::ModelRefused)?;
        let sent = live.link.ask(message);
        match self.answered(sent).await? {
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

    /// Signs in the way `agents.toml` names, when it names one: only a way the agent advertised
    /// as its own to run, and the reply is awaited before `session/new` goes out. The agent reads
    /// its own login; nothing of it passes through us.
    async fn sign_in(&mut self, init: &InitializeResponse) -> Result<(), BackendFault> {
        let live = self.live.as_ref().ok_or(BackendFault::NotRunning)?;
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
        let sent = live
            .link
            .ask(AuthenticateRequest::new(method.as_str().to_owned()));
        // Whatever the agent answers with an error, it did not sign in.
        match self.answered(sent).await? {
            Ok(_) => Ok(()),
            Err(_) => Err(BackendFault::SignInNeeded),
        }
    }
}

/// The fault for an error the agent answered `session/new` with: "authentication required"
/// (-32000) is its own fault, everything else is the agent being unavailable.
fn refusal_fault(error: &Error, methods: Choices) -> BackendFault {
    if error.code != ErrorCode::AuthRequired {
        return Step::NewSession.unavailable(error);
    }
    match methods.0.is_empty() {
        true => BackendFault::SignInNeeded,
        false => BackendFault::SignInChoose(methods),
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
