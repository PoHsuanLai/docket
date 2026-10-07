//! `session/prompt`: the turn loop. It races the host's next event against the editor's lines, so
//! a `session/cancel` is heard while the assistant works, and it asks the editor's person before
//! a call the mode says to ask about. The machine (`turn`) decides; this only carries.

use crate::fault;
use crate::mode::Mode;
use crate::out;
use crate::permission::{self, verdict_of_reply};
use crate::server::{Server, Ticks};
use crate::turn::{Finish, Order, Turn, Wants};
use crate::wire::{Incoming, Wire, WireClosed};
use agent_client_protocol_schema::rpc::RequestId;
use agent_client_protocol_schema::v1::{
    AGENT_METHOD_NAMES, CLIENT_METHOD_NAMES, CancelNotification, ContentBlock, Error,
    PromptRequest, PromptResponse, SessionNotification,
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

/// What woke the loop.
enum Woke {
    Event(Result<Option<BackendEvent>, HostFault>),
    Line(Option<String>),
}

impl<H: SessionHost, L: SessionLog, W: Wire, T: Ticks> Server<H, L, W, T> {
    pub(crate) async fn prompt(
        &mut self,
        id: RequestId,
        params: serde_json::Value,
    ) -> Result<(), WireClosed> {
        match self.begin(params).await {
            Err(error) => self.wire.write_line(out::failure(&id, &error)).await,
            Ok((session, mode)) => {
                let finish = self.drive(&session, Turn::new(mode)).await?;
                let line = match finish {
                    Finish::Stop(reason) => out::reply(&id, &PromptResponse::new(reason)),
                    Finish::Failed => out::failure(&id, &fault::internal("the turn failed")),
                };
                self.wire.write_line(line).await
            }
        }
    }

    /// Records the person's turn and hands it to the host.
    async fn begin(&mut self, params: serde_json::Value) -> Result<(SessionId, Mode), Error> {
        let asked: PromptRequest = fault::params(params)?;
        let session =
            SessionId::parse(&asked.session_id.0).map_err(|_| fault::unknown_session())?;
        let mode = self
            .roster
            .get(&session)
            .ok_or_else(fault::unknown_session)?
            .mode;
        let text = spoken(&asked.prompt).ok_or_else(|| fault::invalid("a prompt needs text"))?;
        let turn = UserTurn {
            id: TurnId(self.mint()),
            text,
            at: self.ticks.now(),
            from: TurnSource::Editor(self.editor.clone()),
            via: TurnVia::Typed,
        };
        self.host.turn(&session, turn).await.map_err(refused)?;
        Ok((session, mode))
    }

    async fn drive(&mut self, session: &SessionId, mut turn: Turn) -> Result<Finish, WireClosed> {
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
                        Woke::Line(Some(line)) => self.heard(session, &line, &mut turn).await?,
                    }
                }
                Wants::Answer(call) => {
                    let asked = RequestId::Str(format!("docket-permission-{}", self.mint()));
                    let request = permission::request(&out::wire_id(session), &call);
                    let line = out::ask(
                        &asked,
                        CLIENT_METHOD_NAMES.session_request_permission,
                        &request,
                    );
                    self.wire.write_line(line).await?;
                    self.await_answer(session, &asked, &mut turn).await?
                }
            };
            self.carry(session, orders).await?;
        }
    }

    /// Reads lines until the editor answers `asked`, or cancels, or goes away.
    async fn await_answer(
        &mut self,
        session: &SessionId,
        asked: &RequestId,
        turn: &mut Turn,
    ) -> Result<Vec<Order>, WireClosed> {
        loop {
            let Some(line) = self.wire.read_line().await else {
                return Ok(turn.cancelled());
            };
            match Incoming::parse(&line) {
                Ok(Incoming::Reply { id, outcome }) if &id == asked => {
                    return Ok(turn.answered(verdict_of_reply(outcome)));
                }
                _ => {
                    let orders = self.heard(session, &line, turn).await?;
                    if !matches!(turn.wants(), Wants::Answer(_)) {
                        return Ok(orders);
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
