//! The ACP server: one editor on one SDK connection, one `SessionHost`, one `SessionLog`. It
//! keeps no authority: every effect a session has goes through the host, which runs our gate
//! (policy, reviewer, taint, breaker, budgets) on every call. The editor's permission prompt sits
//! on top of that gate and can only stop a call, never let one past it.
//!
//! The SDK's agent role does the JSON-RPC: framing, matching replies to requests, and routing
//! each message to a handler by its type. The handlers (`link`) hand what they get to this
//! server's one task, which alone holds the sessions and runs them in order.
//!
//! Methods: `initialize`, `session/new`, `session/load`, `session/list`, `session/prompt`,
//! `session/cancel`, `session/set_mode`. Anything else is `method not found`, which the SDK
//! answers. `mcpServers` in `session/new` and `session/load` are ignored: the editor's tools are
//! not ours to run.

use crate::covered::Covered;
use crate::expose::Permit;
use crate::fault;
use crate::helpers::{Helpers, NoHelpers};
use crate::link::{Gone, Inbound, Link, answer};
use crate::mode::Mode;
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    AgentCapabilities, CancelNotification, Implementation, InitializeRequest, InitializeResponse,
    ListSessionsRequest, LoadSessionRequest, NewSessionRequest, PromptCapabilities, PromptRequest,
    SessionCapabilities, SessionListCapabilities, SetSessionModeRequest, SetSessionModeResponse,
};
use agent_client_protocol::{Agent, ConnectTo, Error, on_receive_notification, on_receive_request};
use companion_wire::ToldWire;
use docket_core::Clock;
use docket_session::{SessionHost, SessionLog};
use futures_channel::mpsc;
use futures_util::future::{Either, select};
use porter_core::AppName;
use prov::{HelperId, SessionId};
use std::collections::BTreeMap;
use std::pin::pin;

/// Where the turns of a session run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Turns {
    /// In this connection's own host.
    Here,
    /// With the helper whose conversation it is, through [`crate::ConversationTurns`].
    AtHelper,
}

/// What the server holds of a session this connection may use.
#[derive(Debug, Clone)]
pub(crate) struct Known {
    pub mode: Mode,
    /// The helper the session is a conversation or a job of; none for the companion's.
    pub helper: Option<HelperId>,
    /// Where its turns run.
    pub turns: Turns,
}

/// What woke the serve loop.
enum Heard {
    Inbound(Option<Inbound>),
    Told(ToldWire),
}

/// Whether `initialize` has happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Readiness {
    Fresh,
    Ready,
}

/// Registers the handler that hands each `$request` the SDK receives to the server's task.
macro_rules! forward {
    ($builder:expr, $tx:expr, $request:ty, $variant:ident) => {{
        let tx = $tx.clone();
        $builder.on_receive_request(
            async move |asked: $request, responder, _cx| {
                tx.unbounded_send(Inbound::$variant(Box::new(asked), responder))
                    .map_err(|_| Error::internal_error())
            },
            on_receive_request!(),
        )
    }};
}

/// The ACP server edge.
#[derive(Debug)]
pub struct Server<H, L, T, S = NoHelpers> {
    pub(crate) host: H,
    pub(crate) log: L,
    pub(crate) ticks: T,
    pub(crate) editor: AppName,
    pub(crate) roster: BTreeMap<SessionId, Known>,
    pub(crate) minted: u64,
    pub(crate) covered: Covered,
    pub(crate) helpers: S,
    readiness: Readiness,
}

impl<H: SessionHost, L: SessionLog, T: Clock> Server<H, L, T, NoHelpers> {
    /// A server for the editor `editor` (the opener its sessions record), allowed to serve by
    /// `permit`.
    pub fn new(_permit: Permit, editor: AppName, host: H, log: L, ticks: T) -> Self {
        Self {
            host,
            log,
            ticks,
            editor,
            roster: BTreeMap::new(),
            minted: 0,
            covered: Covered::default(),
            helpers: NoHelpers,
            readiness: Readiness::Fresh,
        }
    }

    /// The same server that also shows the person's `helpers` to the editor (`session/list`,
    /// `session/load`, `session/new` with `_meta.helper`).
    pub fn with_helpers<S: Helpers>(self, helpers: S) -> Server<H, L, T, S> {
        Server {
            host: self.host,
            log: self.log,
            ticks: self.ticks,
            editor: self.editor,
            roster: self.roster,
            minted: self.minted,
            covered: self.covered,
            helpers,
            readiness: self.readiness,
        }
    }
}

impl<H: SessionHost, L: SessionLog, T: Clock, S: Helpers> Server<H, L, T, S> {
    /// The host, for the caller that built the server to look at after it ran.
    pub fn into_host(self) -> H {
        self.host
    }

    pub(crate) fn mint(&mut self) -> u64 {
        self.minted += 1;
        self.minted
    }

    /// Serves the editor at the other end of `transport` until it closes the connection. Between
    /// its requests, what the board says of a helper the editor has a session for goes out as a
    /// plain line in that session. A sentence that comes while a prompt runs waits for the prompt
    /// to end.
    pub async fn run(&mut self, transport: impl ConnectTo<Agent> + 'static) -> Result<(), Error> {
        let (tx, rx) = mpsc::unbounded();
        let builder = Agent.builder().name("docket-acp");
        let builder = forward!(builder, tx, InitializeRequest, Initialize);
        let builder = forward!(builder, tx, NewSessionRequest, New);
        let builder = forward!(builder, tx, LoadSessionRequest, Load);
        let builder = forward!(builder, tx, ListSessionsRequest, List);
        let builder = forward!(builder, tx, PromptRequest, Prompt);
        let builder = forward!(builder, tx, SetSessionModeRequest, SetMode);
        builder
            .on_receive_notification(
                async move |cancel: CancelNotification, _cx| {
                    tx.unbounded_send(Inbound::Cancel(cancel))
                        .map_err(|_| Error::internal_error())
                },
                on_receive_notification!(),
            )
            .connect_with(transport, async |cx| {
                // An editor that is gone is the normal end.
                let _ = self.serve(&mut Link::new(cx, rx)).await;
                Ok(())
            })
            .await
    }

    async fn serve(&mut self, link: &mut Link) -> Result<(), Gone> {
        loop {
            let watching = self.watching();
            let heard = {
                let inbound = pin!(link.next());
                let told = pin!(self.helpers.next_told(&watching));
                match select(inbound, told).await {
                    Either::Left((inbound, _)) => Heard::Inbound(inbound),
                    Either::Right((told, _)) => Heard::Told(told),
                }
            };
            match heard {
                Heard::Inbound(None) => return Ok(()),
                Heard::Inbound(Some(inbound)) => self.inbound(link, inbound).await?,
                Heard::Told(told) => self.tell(link, told)?,
            }
        }
    }

    async fn inbound(&mut self, link: &mut Link, inbound: Inbound) -> Result<(), Gone> {
        match (self.readiness, inbound) {
            // Nothing to cancel between prompts.
            (_, Inbound::Cancel(_)) => Ok(()),
            (_, Inbound::Initialize(asked, responder)) => {
                answer(responder, self.initialize(*asked))
            }
            (Readiness::Fresh, other) => other.refuse(fault::not_now("initialize first")),
            (Readiness::Ready, Inbound::New(asked, responder)) => {
                answer(responder, self.new_session(*asked).await)
            }
            (Readiness::Ready, Inbound::Load(asked, responder)) => {
                answer(responder, self.load_session(link, *asked).await)
            }
            (Readiness::Ready, Inbound::List(asked, responder)) => {
                answer(responder, self.list_sessions(*asked).await)
            }
            (Readiness::Ready, Inbound::SetMode(asked, responder)) => {
                answer(responder, self.set_mode(*asked))
            }
            (Readiness::Ready, Inbound::Prompt(asked, responder)) => {
                self.prompt(link, *asked, responder).await
            }
        }
    }

    fn initialize(&mut self, _asked: InitializeRequest) -> Result<InitializeResponse, Error> {
        self.readiness = Readiness::Ready;
        // Version 1 whatever the editor asks for: it picks whether to go on.
        let capabilities = AgentCapabilities::new()
            .load_session(true)
            .prompt_capabilities(PromptCapabilities::new())
            .session_capabilities(SessionCapabilities::new().list(SessionListCapabilities::new()));
        Ok(InitializeResponse::new(ProtocolVersion::V1)
            .agent_capabilities(capabilities)
            .agent_info(Implementation::new("docket", env!("CARGO_PKG_VERSION"))))
    }

    fn set_mode(&mut self, asked: SetSessionModeRequest) -> Result<SetSessionModeResponse, Error> {
        let session =
            SessionId::parse(&asked.session_id.0).map_err(|_| fault::unknown_session())?;
        let mode = Mode::parse(&asked.mode_id.0).ok_or_else(|| fault::invalid("no such mode"))?;
        let known = self
            .roster
            .get_mut(&session)
            .ok_or_else(fault::unknown_session)?;
        known.mode = mode;
        Ok(SetSessionModeResponse::new())
    }
}
