//! The ACP server: one editor on one `Wire`, one `SessionHost`, one `SessionLog`. It carries the
//! protocol and keeps no authority: every effect a session has goes through the host, which runs
//! our gate (policy, reviewer, taint, breaker, budgets) on every call. The editor's permission
//! prompt sits on top of that gate and can only stop a call, never let one past it.
//!
//! Methods: `initialize`, `session/new`, `session/load`, `session/list`, `session/prompt`,
//! `session/cancel`, `session/set_mode`. Anything else is `method not found`. `mcpServers` in
//! `session/new` and `session/load` are ignored: the editor's tools are not ours to run.

use crate::expose::Permit;
use crate::fault;
use crate::mode::Mode;
use crate::out;
use crate::wire::{Incoming, Wire, WireClosed};
use agent_client_protocol_schema::ProtocolVersion;
use agent_client_protocol_schema::rpc::RequestId;
use agent_client_protocol_schema::v1::{
    AGENT_METHOD_NAMES, AgentCapabilities, Error, Implementation, InitializeRequest,
    InitializeResponse, PromptCapabilities, SessionCapabilities, SessionListCapabilities,
    SetSessionModeRequest, SetSessionModeResponse,
};
use docket_session::{SessionHost, SessionLog};
use porter_core::AppName;
use prov::{SessionId, UnixSeconds};
use serde_json::Value;
use std::collections::BTreeMap;

/// The time, passed in: the lib reads no clock.
pub trait Ticks: Send {
    /// Now.
    fn now(&self) -> UnixSeconds;
}

/// What the server holds of a session this connection may use.
#[derive(Debug, Clone)]
pub(crate) struct Known {
    pub mode: Mode,
}

/// Whether `initialize` has happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Readiness {
    Fresh,
    Ready,
}

/// The methods we serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Method {
    Initialize,
    New,
    Load,
    List,
    Prompt,
    SetMode,
    Cancel,
}

impl Method {
    fn of(name: &str) -> Option<Method> {
        let names = AGENT_METHOD_NAMES;
        [
            (names.initialize, Method::Initialize),
            (names.session_new, Method::New),
            (names.session_load, Method::Load),
            (names.session_list, Method::List),
            (names.session_prompt, Method::Prompt),
            (names.session_set_mode, Method::SetMode),
            (names.session_cancel, Method::Cancel),
        ]
        .into_iter()
        .find_map(|(n, m)| (n == name).then_some(m))
    }
}

/// The ACP server edge.
#[derive(Debug)]
pub struct Server<H, L, W, T> {
    pub(crate) host: H,
    pub(crate) log: L,
    pub(crate) wire: W,
    pub(crate) ticks: T,
    pub(crate) editor: AppName,
    pub(crate) roster: BTreeMap<SessionId, Known>,
    pub(crate) minted: u64,
    readiness: Readiness,
}

impl<H: SessionHost, L: SessionLog, W: Wire, T: Ticks> Server<H, L, W, T> {
    /// A server for the editor `editor` (the opener its sessions record), allowed to serve by
    /// `permit`.
    pub fn new(_permit: Permit, editor: AppName, host: H, log: L, wire: W, ticks: T) -> Self {
        Self {
            host,
            log,
            wire,
            ticks,
            editor,
            roster: BTreeMap::new(),
            minted: 0,
            readiness: Readiness::Fresh,
        }
    }

    /// The host, for the caller that built the server to look at after it ran.
    pub fn into_host(self) -> H {
        self.host
    }

    pub(crate) fn mint(&mut self) -> u64 {
        self.minted += 1;
        self.minted
    }

    /// Serves until the editor closes the connection.
    pub async fn run(&mut self) -> Result<(), WireClosed> {
        while let Some(line) = self.wire.read_line().await {
            self.line(&line).await?;
        }
        Ok(())
    }

    async fn line(&mut self, line: &str) -> Result<(), WireClosed> {
        match Incoming::parse(line) {
            Err(_) => {
                let bad = out::failure(&RequestId::Null, &Error::parse_error());
                self.wire.write_line(bad).await
            }
            Ok(Incoming::Request { id, method, params }) => self.request(id, &method, params).await,
            Ok(Incoming::Notification { .. } | Incoming::Reply { .. }) => Ok(()),
        }
    }

    async fn request(
        &mut self,
        id: RequestId,
        name: &str,
        params: Value,
    ) -> Result<(), WireClosed> {
        let method = Method::of(name);
        if method == Some(Method::Prompt) && self.readiness == Readiness::Ready {
            return self.prompt(id, params).await;
        }
        let answer = match method {
            None | Some(Method::Cancel) => Err(Error::method_not_found()),
            Some(Method::Initialize) => self.initialize(params),
            Some(_) if self.readiness == Readiness::Fresh => {
                Err(fault::not_now("initialize first"))
            }
            Some(Method::New) => self.new_session(params).await,
            Some(Method::Load) => self.load_session(params).await,
            Some(Method::List) => self.list_sessions(params).await,
            Some(Method::SetMode) => self.set_mode(params),
            Some(Method::Prompt) => Err(fault::not_now("initialize first")),
        };
        let line = match answer {
            Ok(result) => out::reply(&id, &result),
            Err(error) => out::failure(&id, &error),
        };
        self.wire.write_line(line).await
    }

    fn initialize(&mut self, params: Value) -> Result<Value, Error> {
        let _asked: InitializeRequest = fault::params(params)?;
        self.readiness = Readiness::Ready;
        // Version 1 whatever the editor asks for: it picks whether to go on.
        let capabilities = AgentCapabilities::new()
            .load_session(true)
            .prompt_capabilities(PromptCapabilities::new())
            .session_capabilities(SessionCapabilities::new().list(SessionListCapabilities::new()));
        let answer = InitializeResponse::new(ProtocolVersion::V1)
            .agent_capabilities(capabilities)
            .agent_info(Implementation::new("docket", env!("CARGO_PKG_VERSION")));
        serde_json::to_value(answer).map_err(|_| fault::internal("could not answer"))
    }

    fn set_mode(&mut self, params: Value) -> Result<Value, Error> {
        let asked: SetSessionModeRequest = fault::params(params)?;
        let session =
            SessionId::parse(&asked.session_id.0).map_err(|_| fault::unknown_session())?;
        let mode = Mode::parse(&asked.mode_id.0).ok_or_else(|| fault::invalid("no such mode"))?;
        let known = self
            .roster
            .get_mut(&session)
            .ok_or_else(fault::unknown_session)?;
        known.mode = mode;
        serde_json::to_value(SetSessionModeResponse::new())
            .map_err(|_| fault::internal("could not answer"))
    }
}
