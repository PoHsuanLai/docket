//! `Intents` sends the request it is asked for and reads only the reply that request gets.

use docket_client::{ClientError, Intents, Transport, TransportError};
use docket_core::*;
use prov::{ActionName, AppName, SessionId};
use std::collections::VecDeque;
use std::sync::Mutex;

/// A transport that answers from a script and remembers what it was asked.
struct Scripted {
    replies: Mutex<VecDeque<Result<IntentsReply, TransportError>>>,
    seen: Mutex<Vec<IntentsRequest>>,
}

impl Scripted {
    fn answering(replies: Vec<Result<IntentsReply, TransportError>>) -> Self {
        Self {
            replies: Mutex::new(replies.into()),
            seen: Mutex::new(vec![]),
        }
    }
}

impl Transport for Scripted {
    async fn call(&self, request: IntentsRequest) -> Result<IntentsReply, TransportError> {
        self.seen.lock().expect("lock").push(request);
        self.replies
            .lock()
            .expect("lock")
            .pop_front()
            .unwrap_or(Err(TransportError::Closed))
    }
}

fn call() -> CallRequest {
    CallRequest {
        action: ActionRef {
            app: AppName::parse("org.quire.Mail").expect("app"),
            name: ActionName::parse("mail.thread.archive").expect("action"),
        },
        target: TargetValue::Nothing,
        args: Args::new(),
        origin: Origin::Launcher,
    }
}

#[tokio::test]
async fn perform_sends_the_call_and_keeps_the_calls_own_refusal_apart_from_a_refused_request() {
    let denied = IntentsReply::Performed(Box::new(Err(CallRefusal::Denied(DenyCode::NeedsUser))));
    let intents = Intents::over(Scripted::answering(vec![
        Ok(denied),
        Ok(IntentsReply::Refused(WireRefusal::NotAllowed)),
    ]));
    let first = intents.perform(call(), None).await.expect("a reply");
    assert_eq!(first, Err(CallRefusal::Denied(DenyCode::NeedsUser)));
    let second = intents.perform(call(), None).await;
    assert_eq!(second, Err(ClientError::Refused(WireRefusal::NotAllowed)));
}

/// A script the test keeps a handle on.
struct Shared(std::sync::Arc<Scripted>);

impl Transport for Shared {
    async fn call(&self, request: IntentsRequest) -> Result<IntentsReply, TransportError> {
        self.0.call(request).await
    }
}

#[tokio::test]
async fn the_request_the_router_sees_is_the_one_asked_for() {
    let transport = std::sync::Arc::new(Scripted::answering(vec![Ok(IntentsReply::Done)]));
    let intents = Intents::over(Shared(transport.clone()));
    let session = SessionId::parse("s-4").expect("session");
    intents
        .session_close(session.clone())
        .await
        .expect("closed");
    let seen = transport.seen.lock().expect("lock");
    assert_eq!(*seen, [IntentsRequest::SessionClose { session }]);
    assert_eq!(seen[0].member(), Member::SessionClose);
}

#[tokio::test]
async fn a_reply_of_the_wrong_kind_is_unexpected() {
    let intents = Intents::over(Scripted::answering(vec![Ok(IntentsReply::Done)]));
    let got = intents
        .session_display(SessionId::parse("s-1").expect("s"), Handle(1))
        .await;
    assert_eq!(got, Err(ClientError::Unexpected));
}

#[tokio::test]
async fn a_transport_failure_is_a_transport_error() {
    let intents = Intents::over(Scripted::answering(vec![Err(TransportError::Closed)]));
    let got = intents
        .session_close(SessionId::parse("s-1").expect("s"))
        .await;
    assert_eq!(got, Err(ClientError::Transport(TransportError::Closed)));
}

#[tokio::test]
async fn text_for_the_screen_comes_back_as_text() {
    let intents = Intents::over(Scripted::answering(vec![Ok(IntentsReply::Text(
        "hello".into(),
    ))]));
    let got = intents
        .session_display(SessionId::parse("s-1").expect("s"), Handle(1))
        .await;
    assert_eq!(got.as_deref(), Ok("hello"));
}
