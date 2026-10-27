//! The server's side of the SDK's agent role. The SDK frames the JSON-RPC lines, matches replies
//! to requests and routes each message to the handler for its type; the handlers here only hand
//! what they get to the server's one task, through a channel, so that task alone holds the
//! session state. [`Link`] is that task's end: what the editor sent, and the editor to send to.

use crate::out;
use agent_client_protocol::schema::v1::{
    CancelNotification, InitializeRequest, InitializeResponse, ListSessionsRequest,
    ListSessionsResponse, LoadSessionRequest, LoadSessionResponse, NewSessionRequest,
    NewSessionResponse, PromptRequest, PromptResponse, RequestPermissionRequest,
    RequestPermissionResponse, SessionNotification, SessionUpdate, SetSessionModeRequest,
    SetSessionModeResponse,
};
use agent_client_protocol::{
    Client, ConnectionTo, Error, JsonRpcResponse, Responder, is_incoming_transport_closed,
};
use futures_channel::mpsc::UnboundedReceiver;
use futures_channel::oneshot;
use futures_util::StreamExt;
use futures_util::future::{Either, select};
use prov::SessionId;
use std::pin::pin;

/// The editor has gone: nothing more can be said to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the editor closed the connection")]
pub(crate) struct Gone;

/// What the editor sent that the server serves. The requests are boxed: `initialize` alone is
/// over a kilobyte, and every select over the channel would carry that size.
pub(crate) enum Inbound {
    Initialize(Box<InitializeRequest>, Responder<InitializeResponse>),
    New(Box<NewSessionRequest>, Responder<NewSessionResponse>),
    Load(Box<LoadSessionRequest>, Responder<LoadSessionResponse>),
    List(Box<ListSessionsRequest>, Responder<ListSessionsResponse>),
    Prompt(Box<PromptRequest>, Responder<PromptResponse>),
    SetMode(
        Box<SetSessionModeRequest>,
        Responder<SetSessionModeResponse>,
    ),
    Cancel(CancelNotification),
}

/// Answers `responder` with `result`.
pub(crate) fn answer<T: JsonRpcResponse>(
    responder: Responder<T>,
    result: Result<T, Error>,
) -> Result<(), Gone> {
    responder.respond_with_result(result).map_err(|_| Gone)
}

impl Inbound {
    /// Turns the request away with `error`. A cancel has no one to answer.
    pub(crate) fn refuse(self, error: Error) -> Result<(), Gone> {
        match self {
            Inbound::Initialize(_, responder) => answer(responder, Err(error)),
            Inbound::New(_, responder) => answer(responder, Err(error)),
            Inbound::Load(_, responder) => answer(responder, Err(error)),
            Inbound::List(_, responder) => answer(responder, Err(error)),
            Inbound::Prompt(_, responder) => answer(responder, Err(error)),
            Inbound::SetMode(_, responder) => answer(responder, Err(error)),
            Inbound::Cancel(_) => Ok(()),
        }
    }
}

/// What came of asking the editor.
pub(crate) enum Settled {
    /// Its answer, or the error it answered with.
    Answer(Result<RequestPermissionResponse, Error>),
    /// The editor went before it answered.
    Gone,
}

/// The editor: its messages in, ours out.
pub(crate) struct Link {
    cx: ConnectionTo<Client>,
    rx: UnboundedReceiver<Inbound>,
}

impl Link {
    pub(crate) fn new(cx: ConnectionTo<Client>, rx: UnboundedReceiver<Inbound>) -> Self {
        Self { cx, rx }
    }

    /// The next message from the editor; `None` once it has closed the connection and everything
    /// it sent has been taken. Cancel-safe: a message is not lost when this is dropped.
    pub(crate) async fn next(&mut self) -> Option<Inbound> {
        let heard = {
            let closed = pin!(self.cx.incoming_closed());
            match select(self.rx.next(), closed).await {
                Either::Left((inbound, _)) => Some(inbound),
                Either::Right(_) => None,
            }
        };
        match heard {
            Some(inbound) => inbound,
            // What the SDK dispatched before the close is already queued.
            None => self.rx.try_recv().ok(),
        }
    }

    /// Sends `session/update` for `session`.
    pub(crate) fn update(&self, session: &SessionId, update: SessionUpdate) -> Result<(), Gone> {
        let note = SessionNotification::new(out::wire_id(session), update);
        self.cx.send_notification(note).map_err(|_| Gone)
    }

    /// Sends the permission `request`. Its answer comes on the returned channel; dropping the
    /// channel drops the answer, and the editor is not told to cancel the request.
    pub(crate) fn ask(
        &self,
        request: RequestPermissionRequest,
    ) -> oneshot::Receiver<Result<RequestPermissionResponse, Error>> {
        let (tx, rx) = oneshot::channel();
        let sent = self
            .cx
            .send_request(request)
            .on_receiving_result(move |result| async move {
                let _ = tx.send(result);
                Ok(())
            });
        if let Err(error) = sent {
            tracing::warn!(%error, "could not ask the editor");
        }
        rx
    }
}

/// Whether the editor went rather than answered.
pub(crate) fn settled(
    result: Result<Result<RequestPermissionResponse, Error>, oneshot::Canceled>,
) -> Settled {
    match result {
        Ok(Err(error)) if is_incoming_transport_closed(&error) => Settled::Gone,
        Ok(answer) => Settled::Answer(answer),
        Err(oneshot::Canceled) => Settled::Gone,
    }
}
