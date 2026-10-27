//! `session/prompt`: the turn loop. It races the host's next event against the editor's messages,
//! so a `session/cancel` is heard while the assistant works, and it asks the editor's person
//! before a call the mode says to ask about. The machine (`turn`) decides; this only carries.

use crate::calls;
use crate::fault;
use crate::helpers::Helpers;
use crate::link::{Gone, Inbound, Link, Settled, answer, settled};
use crate::mode::Mode;
use crate::out;
use crate::permission;
use crate::server::{Server, Turns};
use crate::turn::{Finish, Order, Turn, Wants};
use acp_core::permission::verdict_of_reply;
use agent_client_protocol::Responder;
use agent_client_protocol::schema::v1::{
    ContentBlock, Error, PromptRequest, PromptResponse, RequestPermissionRequest,
    RequestPermissionResponse, StopReason,
};
use docket_core::Clock;
use docket_core::{TurnId, TurnSource, TurnVia, UserTurn};
use docket_session::{BackendEvent, HostFault, SessionHost, SessionLog, TurnEnd};
use futures_channel::oneshot;
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
    Answer(Result<RequestPermissionResponse, Error>),
    /// A cancel or a hang-up ended the wait first; these are the orders it brings.
    Heard(Vec<Order>),
}

/// What ended a wait on the editor.
enum Listened {
    Reply(Result<Result<RequestPermissionResponse, Error>, oneshot::Canceled>),
    Inbound(Option<Inbound>),
}

/// What woke the loop.
enum Woke {
    Event(Result<Option<BackendEvent>, HostFault>),
    Inbound(Option<Inbound>),
}

/// A message that arrived while a turn runs: a cancel for this session stops it, any other
/// request is turned away.
fn heard(session: &SessionId, inbound: Inbound, turn: &mut Turn) -> Result<Vec<Order>, Gone> {
    match inbound {
        Inbound::Cancel(cancel) => Ok(if &*cancel.session_id.0 == session.as_str() {
            turn.cancelled()
        } else {
            Vec::new()
        }),
        other => other
            .refuse(fault::not_now("a prompt is in progress"))
            .map(|()| Vec::new()),
    }
}

impl<H: SessionHost, L: SessionLog, T: Clock, S: Helpers> Server<H, L, T, S> {
    pub(crate) async fn prompt(
        &mut self,
        link: &mut Link,
        asked: PromptRequest,
        responder: Responder<PromptResponse>,
    ) -> Result<(), Gone> {
        match self.begin(asked).await {
            Err(error) => answer(responder, Err(error)),
            Ok(Begun::AtHelper(session, turn)) => {
                self.converse(link, responder, session, turn).await
            }
            Ok(Begun::Here(session, mode)) => {
                let mut turn = Turn::new(mode, self.covered.clone());
                let finish = self.drive(link, &session, &mut turn).await;
                self.covered = turn.covered();
                let result = match finish? {
                    Finish::Stop(reason) => Ok(PromptResponse::new(reason)),
                    Finish::Failed => Err(fault::internal("the turn failed")),
                };
                answer(responder, result)
            }
        }
    }

    /// Reads the prompt, and either hands the person's turn to the host or, for a helper's
    /// conversation, keeps it to be taken to the helper.
    async fn begin(&mut self, asked: PromptRequest) -> Result<Begun, Error> {
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
            mode: None,
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
        link: &Link,
        responder: Responder<PromptResponse>,
        session: SessionId,
        turn: UserTurn,
    ) -> Result<(), Gone> {
        let result = match self.helpers.say(&session, turn).await {
            Ok(words) => {
                if !words.trim().is_empty() {
                    link.update(&session, calls::say(words))?;
                }
                Ok(PromptResponse::new(StopReason::EndTurn))
            }
            Err(refusal) => Err(fault::internal(&refusal.0)),
        };
        answer(responder, result)
    }

    async fn drive(
        &mut self,
        link: &mut Link,
        session: &SessionId,
        turn: &mut Turn,
    ) -> Result<Finish, Gone> {
        loop {
            let orders = match turn.wants() {
                Wants::Over(finish) => return Ok(finish),
                Wants::Event => {
                    let woke = {
                        let event = pin!(self.host.next_event(session));
                        let inbound = pin!(link.next());
                        match select(event, inbound).await {
                            Either::Left((event, _)) => Woke::Event(event),
                            Either::Right((inbound, _)) => Woke::Inbound(inbound),
                        }
                    };
                    match woke {
                        Woke::Event(Ok(Some(event))) => turn.event(event),
                        Woke::Event(_) => turn.event(BackendEvent::TurnEnd(TurnEnd::Failed)),
                        Woke::Inbound(None) => turn.cancelled(),
                        Woke::Inbound(Some(inbound)) => heard(session, inbound, turn)?,
                    }
                }
                Wants::Answer(call) => {
                    let request = permission::request(&out::wire_id(session), &call);
                    match self.ask_editor(link, session, request, turn).await? {
                        Reply::Heard(orders) => orders,
                        Reply::Answer(reply) => turn.answered(verdict_of_reply(reply)),
                    }
                }
                Wants::Confirm(sheet) => self.put_sheet(link, session, &sheet, turn).await?,
            };
            self.carry(link, session, orders).await?;
        }
    }

    /// Sends the permission `request` and listens until the editor answers it, or cancels, or
    /// goes away.
    pub(crate) async fn ask_editor(
        &mut self,
        link: &mut Link,
        session: &SessionId,
        request: RequestPermissionRequest,
        turn: &mut Turn,
    ) -> Result<Reply, Gone> {
        let mut waiting = link.ask(request);
        loop {
            let listened = {
                let inbound = pin!(link.next());
                match select(&mut waiting, inbound).await {
                    Either::Left((reply, _)) => Listened::Reply(reply),
                    Either::Right((inbound, _)) => Listened::Inbound(inbound),
                }
            };
            match listened {
                Listened::Reply(reply) => {
                    return Ok(match settled(reply) {
                        Settled::Answer(reply) => Reply::Answer(reply),
                        Settled::Gone => Reply::Heard(turn.cancelled()),
                    });
                }
                Listened::Inbound(None) => return Ok(Reply::Heard(turn.cancelled())),
                Listened::Inbound(Some(inbound)) => {
                    let orders = heard(session, inbound, turn)?;
                    if !matches!(turn.wants(), Wants::Answer(_) | Wants::Confirm(_)) {
                        return Ok(Reply::Heard(orders));
                    }
                }
            }
        }
    }

    async fn carry(
        &mut self,
        link: &Link,
        session: &SessionId,
        orders: Vec<Order>,
    ) -> Result<(), Gone> {
        for order in orders {
            match order {
                Order::Update(update) => link.update(session, *update)?,
                Order::CancelBackend => {
                    // A host that cannot cancel still ends the turn on its own terms.
                    let _ = self.host.cancel(session).await;
                }
            }
        }
        Ok(())
    }
}
