//! The client edge's side of the SDK's client role. The SDK frames the JSON-RPC lines, matches
//! replies to the requests we sent and routes each message the agent sends to the handler for its
//! type (a method it has no handler for is answered "method not found", one whose parameters do
//! not fit is answered "invalid params"). The handlers here only hand what they get to the
//! backend, through one channel, so the backend alone holds the state and the order is the order
//! the agent sent them in. [`Link`] is the backend's end: what the agent sent, and the agent to
//! send to.
//!
//! The connection runs as a task of its own, so that the agent is heard between the backend's
//! calls. It ends when the agent closes its output (or sends a line over the cap), and is stopped
//! when the link is dropped.

use acp_core::fault;
use agent_client_protocol::schema::v1::{
    CancelNotification, CreateElicitationRequest, CreateTerminalRequest, KillTerminalRequest,
    PromptRequest, PromptResponse, ReadTextFileRequest, ReleaseTerminalRequest,
    RequestPermissionRequest, SessionId, SessionNotification, TerminalOutputRequest,
    WaitForTerminalExitRequest, WriteTextFileRequest,
};
use agent_client_protocol::{
    Agent, Client, ConnectTo, ConnectionTo, Error, JsonRpcRequest, Responder, SentRequest,
    is_connection_terminated, is_incoming_transport_closed, on_receive_notification,
    on_receive_request,
};
use futures_channel::{mpsc, oneshot};
use futures_util::future::{Either, select};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::pin::pin;
use tokio::task::JoinHandle;

/// How many messages from the agent wait for the backend. Past it the SDK stops reading the
/// agent's output, which holds the agent back as a full pipe did.
const DEPTH: usize = 32;

/// The agent has closed the connection, or it ended: nothing more can be said to it or heard from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the agent closed the connection")]
pub(super) struct Gone;

/// How to answer one request of the agent's. The answer is held as JSON and checked by the SDK
/// against the response type of the method.
pub(super) type Reply = Responder<Value>;

/// A request the router rules on, one at a time.
#[derive(Debug, Clone)]
pub(super) enum Work {
    /// `fs/read_text_file`.
    Read(ReadTextFileRequest),
    /// `fs/write_text_file`.
    Write(WriteTextFileRequest),
    /// `session/request_permission`.
    Permission(Box<RequestPermissionRequest>),
    /// `terminal/create`: a command, announced as a call.
    Create(Box<CreateTerminalRequest>),
}

/// A request answered without the router.
#[derive(Debug)]
pub(super) enum Plain {
    /// `terminal/output`.
    Output(TerminalOutputRequest),
    /// `terminal/wait_for_exit`: answered when the command has ended.
    Wait(WaitForTerminalExitRequest),
    /// `terminal/kill`.
    Kill(KillTerminalRequest),
    /// `terminal/release`.
    Release(ReleaseTerminalRequest),
    /// `elicitation/create`: declined, we offer none.
    Elicitation,
}

impl Work {
    fn permission(asked: RequestPermissionRequest) -> Self {
        Work::Permission(Box::new(asked))
    }

    fn create(asked: CreateTerminalRequest) -> Self {
        Work::Create(Box::new(asked))
    }
}

impl Plain {
    /// Whatever the form asks, the answer is no.
    fn declined(_asked: CreateElicitationRequest) -> Self {
        Plain::Elicitation
    }
}

/// What the agent sent, or the answer to the prompt we sent it.
pub(super) enum FromAgent {
    /// A `session/update`.
    Update(Box<SessionNotification>),
    /// A request the router rules on.
    Ruled(Work, Reply),
    /// A request answered at once.
    Plain(Plain, Reply),
    /// The answer to `session/prompt`, in order with what the agent sent before it.
    Prompt(Result<PromptResponse, Error>),
}

/// Registers the handler that hands each `$request` the SDK receives to the backend as
/// `FromAgent::$kind($make(request), reply)`.
macro_rules! forward {
    ($builder:expr, $tx:expr, $request:ty, $kind:ident, $make:path) => {{
        let tx = $tx.clone();
        $builder.on_receive_request(
            async move |asked: $request, responder, _cx| {
                tx.clone()
                    .send(FromAgent::$kind($make(asked), responder.erase_to_json()))
                    .await
                    .map_err(|_| Error::internal_error())
            },
            on_receive_request!(),
        )
    }};
}

/// The agent: its messages in, ours out.
pub(super) struct Link {
    cx: ConnectionTo<Agent>,
    rx: mpsc::Receiver<FromAgent>,
    tx: mpsc::Sender<FromAgent>,
    task: JoinHandle<()>,
}

impl Drop for Link {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Link {
    /// Connects the client role to the agent at the other end of `transport`.
    pub(super) async fn connect(transport: impl ConnectTo<Client>) -> Result<Self, Gone> {
        let (tx, rx) = mpsc::channel(DEPTH);
        let (up_tx, up_rx) = oneshot::channel();
        let builder = Client.builder().name("docket");
        let builder = forward!(builder, tx, ReadTextFileRequest, Ruled, Work::Read);
        let builder = forward!(builder, tx, WriteTextFileRequest, Ruled, Work::Write);
        let builder = forward!(
            builder,
            tx,
            RequestPermissionRequest,
            Ruled,
            Work::permission
        );
        let builder = forward!(builder, tx, CreateTerminalRequest, Ruled, Work::create);
        let builder = forward!(builder, tx, TerminalOutputRequest, Plain, Plain::Output);
        let builder = forward!(builder, tx, WaitForTerminalExitRequest, Plain, Plain::Wait);
        let builder = forward!(builder, tx, KillTerminalRequest, Plain, Plain::Kill);
        let builder = forward!(builder, tx, ReleaseTerminalRequest, Plain, Plain::Release);
        let builder = forward!(
            builder,
            tx,
            CreateElicitationRequest,
            Plain,
            Plain::declined
        );
        let updates = tx.clone();
        let builder = builder.on_receive_notification(
            async move |note: SessionNotification, _cx| {
                updates
                    .clone()
                    .send(FromAgent::Update(Box::new(note)))
                    .await
                    .map_err(|_| Error::internal_error())
            },
            on_receive_notification!(),
        );
        let task = tokio::spawn(async move {
            let ended = builder
                .connect_with(transport, async move |cx| {
                    let _ = up_tx.send(cx.clone());
                    // The connection is over when the agent has closed its output; the handlers
                    // go with it, and the backend hears the end after what was queued.
                    cx.incoming_closed().await;
                    Ok(())
                })
                .await;
            if let Err(error) = ended {
                tracing::debug!(%error, "the agent connection ended with an error");
            }
        });
        let cx = up_rx.await.map_err(|_| Gone)?;
        Ok(Self { cx, rx, tx, task })
    }

    /// The next message from the agent; `None` once the connection has ended and everything the
    /// agent sent has been taken. Cancel-safe: a message is not lost when this is dropped.
    pub(super) async fn next(&mut self) -> Option<FromAgent> {
        if self.task.is_finished() {
            return self.rx.try_recv().ok();
        }
        let heard = match select(self.rx.next(), &mut self.task).await {
            Either::Left((heard, _)) => Some(heard),
            Either::Right(_) => None,
        };
        match heard {
            Some(heard) => heard,
            // The connection ended: the SDK had dispatched everything the agent sent before the
            // end, and that is queued.
            None => self.rx.try_recv().ok(),
        }
    }

    /// Sends `request`; its answer is read with [`Link::answer`].
    pub(super) fn ask<R: JsonRpcRequest>(&self, request: R) -> SentRequest<R::Response> {
        self.cx.send_request(request)
    }

    /// Sends `session/prompt`. Its answer comes through [`Link::next`] as `FromAgent::Prompt`,
    /// after the updates the agent sent before it.
    pub(super) fn prompt(&self, request: PromptRequest) -> Result<(), Gone> {
        let mut tx = self.tx.clone();
        self.cx
            .send_request(request)
            .on_receiving_result(move |result| async move {
                tx.send(FromAgent::Prompt(result))
                    .await
                    .map_err(|_| Error::internal_error())
            })
            .map_err(|_| Gone)
    }

    /// Sends `session/cancel` for the agent's `session`.
    pub(super) fn cancel(&self, session: &SessionId) -> Result<(), Gone> {
        self.cx
            .send_notification(CancelNotification::new(session.clone()))
            .map_err(|_| Gone)
    }

    /// Waits for the answer to `sent`, which is either the agent's result or the error it answered
    /// with. Meanwhile what the agent asks is turned away ("not now") and what it reports is
    /// dropped: before a session exists, and between turns, it may ask for nothing.
    pub(super) async fn answer<T: Send>(
        &mut self,
        sent: SentRequest<T>,
    ) -> Result<Result<T, Error>, Gone> {
        let mut answer = pin!(sent.block_task());
        loop {
            match select(answer.as_mut(), self.rx.next()).await {
                Either::Left((Err(error), _))
                    if is_incoming_transport_closed(&error) || is_connection_terminated(&error) =>
                {
                    return Err(Gone);
                }
                Either::Left((result, _)) => return Ok(result),
                Either::Right((
                    Some(FromAgent::Ruled(_, reply) | FromAgent::Plain(_, reply)),
                    _,
                )) => {
                    let _ = reply.respond_with_error(fault::not_now("not now"));
                }
                Either::Right((Some(_), _)) => {}
                Either::Right((None, _)) => return Err(Gone),
            }
        }
    }
}
