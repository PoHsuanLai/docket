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

fn ask_for_run() -> CuaAsk {
    CuaAsk {
        run: prov::RunId::parse("r-1").expect("run"),
        step: 1,
        app: AppName::parse("org.quire.Mail").expect("app"),
        trust: WindowTrust::Quire,
        mode: RunMode::InPlace,
        space: prov::SpaceId::parse("work").expect("space"),
        action: cua_action::CuaAction::<cua_action::WindowSpace>::Observe,
        node: None,
        effect: prov::Effect::Read,
        basis: EffectBasis::DefaultTable,
        screen: prov::Label::trusted_user(),
    }
}

#[tokio::test]
async fn the_gate_methods_send_their_ask_and_read_only_their_own_reply() {
    let intents = Intents::over(Scripted::answering(vec![
        Ok(IntentsReply::Granted(GrantAnswer::Granted)),
        Ok(IntentsReply::Gate(GateAnswer::Run)),
        Ok(IntentsReply::Done),
        Ok(IntentsReply::Granted(GrantAnswer::Granted)),
    ]));
    let grant = GrantAsk {
        app: AppName::parse("org.quire.Mail").expect("app"),
        space: prov::SpaceId::parse("work").expect("space"),
    };
    assert_eq!(
        intents.gate_grant(grant.clone()).await,
        Ok(GrantAnswer::Granted)
    );
    assert_eq!(intents.gate_check(ask_for_run()).await, Ok(GateAnswer::Run));
    assert_eq!(
        intents.gate_check(ask_for_run()).await,
        Err(ClientError::Unexpected),
        "a reply for another request is not a gate answer"
    );
    assert_eq!(
        intents.gate_check(ask_for_run()).await,
        Err(ClientError::Unexpected)
    );
}

#[tokio::test]
async fn perform_sends_the_call_and_keeps_the_calls_own_refusal_apart_from_a_refused_request() {
    let denied = IntentsReply::Performed(Box::new(Err(CallRefusal::Denied(DenyCode::NeedsUser))));
    let intents = Intents::over(Scripted::answering(vec![
        Ok(denied),
        Ok(IntentsReply::Refused(WireRefusal::NotAllowed)),
    ]));
    let first = intents.perform(call(), None, None).await.expect("a reply");
    assert_eq!(first, Err(CallRefusal::Denied(DenyCode::NeedsUser)));
    let second = intents.perform(call(), None, None).await;
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
    let transport = std::sync::Arc::new(Scripted::answering(vec![
        Ok(IntentsReply::Done),
        Ok(IntentsReply::Done),
    ]));
    let intents = Intents::over(Shared(transport.clone()));
    let session = SessionId::parse("s-4").expect("session");
    intents
        .session_close(session.clone())
        .await
        .expect("closed");
    intents
        .session_turn_ended(session.clone(), TurnId(2), TurnEnd::Failed)
        .await
        .expect("ended");
    let seen = transport.seen.lock().expect("lock");
    let ended = IntentsRequest::SessionTurnEnded {
        session: session.clone(),
        turn: TurnId(2),
        how: TurnEnd::Failed,
    };
    assert_eq!(*seen, [IntentsRequest::SessionClose { session }, ended]);
    assert_eq!(seen[0].member(), Member::SessionClose);
    assert_eq!(seen[1].member(), Member::SessionTurnEnded);
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

#[tokio::test]
async fn text_for_the_screen_can_come_back_with_its_label() {
    let shown = Displayed {
        text: "hello".into(),
        label: prov::Label::trusted_user(),
    };
    let intents = Intents::over(Scripted::answering(vec![
        Ok(IntentsReply::Displayed(shown.clone())),
        Ok(IntentsReply::Text("hello".into())),
    ]));
    let session = SessionId::parse("s-1").expect("s");
    assert_eq!(
        intents
            .session_display_labelled(session.clone(), Handle(1))
            .await,
        Ok(shown)
    );
    assert_eq!(
        intents.session_display_labelled(session, Handle(1)).await,
        Err(ClientError::Unexpected),
        "plain text is not a labelled reply"
    );
}

#[tokio::test]
async fn a_transport_that_cannot_watch_gives_the_verdict_alone_and_proceed_does_nothing() {
    use docket_client::GateEvent;
    let intents = Intents::over(Scripted::answering(vec![
        Ok(IntentsReply::Gate(GateAnswer::Run)),
        Ok(IntentsReply::Refused(WireRefusal::NotAllowed)),
        Ok(IntentsReply::Done),
    ]));
    let mut watch = intents
        .gate_check_watched(ask_for_run())
        .await
        .expect("a watch");
    assert_eq!(watch.proceed().await, Ok(()));
    assert_eq!(watch.next().await, Ok(GateEvent::Verdict(GateAnswer::Run)));
    assert_eq!(
        watch.next().await,
        Err(ClientError::Transport(TransportError::Closed)),
        "nothing follows a verdict"
    );
    let mut refused = intents
        .gate_check_watched(ask_for_run())
        .await
        .expect("a watch");
    assert_eq!(
        refused.next().await,
        Err(ClientError::Refused(WireRefusal::NotAllowed))
    );
    let mut wrong = intents
        .gate_check_watched(ask_for_run())
        .await
        .expect("a watch");
    assert_eq!(wrong.next().await, Err(ClientError::Unexpected));
}
