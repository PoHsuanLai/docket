//! `session/prompt`: the turn loop. It races the host's next event against the editor's lines, so
//! a `session/cancel` is heard while the assistant works, and it asks the editor's person before
//! a call the mode says to ask about. The machine (`turn`) decides; this only carries.

use crate::calls;
use crate::fault;
use crate::helpers::Helpers;
use crate::mode::Mode;
use crate::out;
use crate::permission::{self, verdict_of_reply};
use crate::server::{Server, Ticks, Turns};
use crate::turn::{Finish, Order, Turn, Wants};
use crate::wire::{Incoming, Wire, WireClosed};
use agent_client_protocol_schema::rpc::RequestId;
use agent_client_protocol_schema::v1::{
    AGENT_METHOD_NAMES, CLIENT_METHOD_NAMES, CancelNotification, ContentBlock, Error,
    PromptRequest, PromptResponse, RequestPermissionRequest, SessionNotification, StopReason,
};
use docket_core::{TurnId, TurnSource, TurnVia, UserTurn};
use docket_session::{BackendEvent, HostFault, SessionHost, SessionLog, TurnEnd};
use futures_util::future::{Either, select};
use prov::SessionId;
use std::pin::pin;

/// The person's own words in a prompt: its text blocks. Everything else the editor attaches
/// (files, links, images) is data it chose, not words the person said, and is not recorded as a
/// turn, so it cannot write the task's policy.
fn spoken(blocks: &[ContentBlock]) -> Option<String> {
    let text: Vec<&str> = blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect();
    let joined = text.join("\n");
    (!joined.trim().is_empty()).then_some(joined)
}

fn refused(fault: HostFault) -> Error {
    match fault {
        HostFault::NotOpen => fault::not_now("the session takes no turn"),
        HostFault::NoSuchSession => fault::unknown_session(),
        _ => fault::internal("the turn could not be started"),
    }
}

/// A prompt that was accepted, and where its turn runs.
enum Begun {
    /// In this connection's host, in this mode.
    Here(SessionId, Mode),
    /// With the helper, whose conversation it is.
    AtHelper(SessionId, UserTurn),
}

/// What came of asking the editor.
pub(crate) enum Reply {
    /// Its answer.
    Answer(Result<serde_json::Value, serde_json::Value>),
    /// A cancel or a hang-up ended the wait first; these are the orders it brings.
    Heard(Vec<Order>),
}

/// What woke the loop.
enum Woke {
    Event(Result<Option<BackendEvent>, HostFault>),
    Line(Option<String>),
}

impl<H: SessionHost, L: SessionLog, W: Wire, T: Ticks, S: Helpers> Server<H, L, W, T, S> {
    pub(crate) async fn prompt(
        &mut self,
        id: RequestId,
        params: serde_json::Value,
    ) -> Result<(), WireClosed> {
        match self.begin(params).await {
            Err(error) => self.wire.write_line(out::failure(&id, &error)).await,
            Ok(Begun::AtHelper(session, turn)) => self.converse(id, session, turn).await,
            Ok(Begun::Here(session, mode)) => {
                let mut turn = Turn::new(mode, self.covered.clone());
                let finish = self.drive(&session, &mut turn).await;
                self.covered = turn.covered();
                let finish = finish?;
                let line = match finish {
                    Finish::Stop(reason) => out::reply(&id, &PromptResponse::new(reason)),
                    Finish::Failed => out::failure(&id, &fault::internal("the turn failed")),
                };
                self.wire.write_line(line).await
            }
        }
    }

    /// Reads the prompt, and either hands the person's turn to the host or, for a helper's
    /// conversation, keeps it to be taken to the helper.
    async fn begin(&mut self, params: serde_json::Value) -> Result<Begun, Error> {
        let asked: PromptRequest = fault::params(params)?;
        let session =
            SessionId::parse(&asked.session_id.0).map_err(|_| fault::unknown_session())?;
        let known = self
            .roster
            .get(&session)
            .ok_or_else(fault::unknown_session)?;
        let (mode, turns) = (known.mode, known.turns);
        let text = spoken(&asked.prompt).ok_or_else(|| fault::invalid("a prompt needs text"))?;
        let turn = UserTurn {
            id: TurnId(self.mint()),
            text,
            at: self.ticks.now(),
            from: TurnSource::Editor(self.editor.clone()),
            via: TurnVia::Typed,
        };
        if turns == Turns::AtHelper {
            return Ok(Begun::AtHelper(session, turn));
        }
        self.host.turn(&session, turn).await.map_err(refused)?;
        Ok(Begun::Here(session, mode))
    }

    /// A turn in a helper's conversation: the person's words go to the helper, whose words come
    /// back as one agent line, and the prompt ends. A helper that could not answer is a plain
    /// error in its own sentence.
    async fn converse(
        &mut self,
        id: RequestId,
        session: SessionId,
        turn: UserTurn,
    ) -> Result<(), WireClosed> {
        let line = match self.helpers.say(&session, turn).await {
            Ok(words) => {
                if !words.trim().is_empty() {
                    let note = SessionNotification::new(out::wire_id(&session), calls::say(words));
                    let update = out::notify(CLIENT_METHOD_NAMES.session_update, &note);
                    self.wire.write_line(update).await?;
                }
                out::reply(&id, &PromptResponse::new(StopReason::EndTurn))
            }
            Err(refusal) => out::failure(&id, &fault::internal(&refusal.0)),
        };
        self.wire.write_line(line).await
    }

    async fn drive(&mut self, session: &SessionId, turn: &mut Turn) -> Result<Finish, WireClosed> {
        loop {
            let orders = match turn.wants() {
                Wants::Over(finish) => return Ok(finish),
                Wants::Event => {
                    let woke = {
                        let event = pin!(self.host.next_event(session));
                        let line = pin!(self.wire.read_line());
                        match select(event, line).await {
                            Either::Left((event, _)) => Woke::Event(event),
                            Either::Right((line, _)) => Woke::Line(line),
                        }
                    };
                    match woke {
                        Woke::Event(Ok(Some(event))) => turn.event(event),
                        Woke::Event(_) => turn.event(BackendEvent::TurnEnd(TurnEnd::Failed)),
                        Woke::Line(None) => turn.cancelled(),
                        Woke::Line(Some(line)) => self.heard(session, &line, turn).await?,
                    }
                }
                Wants::Answer(call) => {
                    let request = permission::request(&out::wire_id(session), &call);
                    match self.ask_editor(session, &request, turn).await? {
                        Reply::Heard(orders) => orders,
                        Reply::Answer(reply) => turn.answered(verdict_of_reply(reply)),
                    }
                }
                Wants::Sheet(sheet) => self.put_sheet(session, &sheet, turn).await?,
            };
            self.carry(session, orders).await?;
        }
    }

    /// Sends the permission `request` and reads lines until the editor answers it, or cancels,
    /// or goes away.
    pub(crate) async fn ask_editor(
        &mut self,
        session: &SessionId,
        request: &RequestPermissionRequest,
        turn: &mut Turn,
    ) -> Result<Reply, WireClosed> {
        let asked = RequestId::Str(format!("docket-permission-{}", self.mint()));
        let line = out::ask(
            &asked,
            CLIENT_METHOD_NAMES.session_request_permission,
            request,
        );
        self.wire.write_line(line).await?;
        loop {
            let Some(line) = self.wire.read_line().await else {
                return Ok(Reply::Heard(turn.cancelled()));
            };
            match Incoming::parse(&line) {
                Ok(Incoming::Reply { id, outcome }) if id == asked => {
                    return Ok(Reply::Answer(outcome));
                }
                _ => {
                    let orders = self.heard(session, &line, turn).await?;
                    if !matches!(turn.wants(), Wants::Answer(_) | Wants::Sheet(_)) {
                        return Ok(Reply::Heard(orders));
                    }
                }
            }
        }
    }

    /// A line that arrived while a turn runs: a cancel for this session stops it, any other
    /// request is turned away, anything else is ignored.
    async fn heard(
        &mut self,
        session: &SessionId,
        line: &str,
        turn: &mut Turn,
    ) -> Result<Vec<Order>, WireClosed> {
        match Incoming::parse(line) {
            Ok(Incoming::Notification { method, params })
                if method == AGENT_METHOD_NAMES.session_cancel =>
            {
                let ours = serde_json::from_value::<CancelNotification>(params)
                    .is_ok_and(|c| &*c.session_id.0 == session.as_str());
                Ok(if ours { turn.cancelled() } else { Vec::new() })
            }
            Ok(Incoming::Request { id, .. }) => {
                let busy = fault::not_now("a prompt is in progress");
                self.wire.write_line(out::failure(&id, &busy)).await?;
                Ok(Vec::new())
            }
            _ => Ok(Vec::new()),
        }
    }

    async fn carry(&mut self, session: &SessionId, orders: Vec<Order>) -> Result<(), WireClosed> {
        for order in orders {
            match order {
                Order::Update(update) => {
                    let note = SessionNotification::new(out::wire_id(session), *update);
                    let line = out::notify(CLIENT_METHOD_NAMES.session_update, &note);
                    self.wire.write_line(line).await?;
                }
                Order::CancelBackend => {
                    // A host that cannot cancel still ends the turn on its own terms.
                    let _ = self.host.cancel(session).await;
                }
            }
        }
        Ok(())
    }
}
