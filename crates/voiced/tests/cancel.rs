//! `Utterance.Cancel(cause)`: the caller's cause reaches `Ended` unchanged; the daemon's own
//! causes are refused. Private bus, fake device, no clock.

mod support;

use support::infer::ScriptedInfer;
use support::*;
use voice_wire::{CancelCause, UtteranceEnd, VoiceEvent};
use voiced::UtteranceProxy;

const ALLOWED: [CancelCause; 4] = [
    CancelCause::Escape,
    CancelCause::OtherInput,
    CancelCause::FocusLost,
    CancelCause::Shell,
];

fn is_malformed(error: &zbus::Error) -> bool {
    matches!(error, zbus::Error::MethodError(name, _, _)
        if name.as_str() == "org.quire.Voice1.Error.Malformed")
}

#[tokio::test]
async fn the_begin_caller_cancels_with_each_allowed_cause() {
    for cause in ALLOWED {
        let world = world(Options::new(ScriptedInfer::new(vec![transcript(
            "a", "b", "c",
        )])))
        .await;
        let mut ended = ended_stream(&world.shell).await;
        let (utterance, mut events) = start_utterance(&world, VoiceIntent::Ask).await;
        assert_eq!(events.next().await, Some(VoiceEvent::Opened));
        utterance.cancel(&cause_wire(cause)).await.expect("cancel");
        let seen = events.until_ended().await;
        assert_eq!(ended_of(&seen), UtteranceEnd::Cancelled(cause));
        assert_eq!(
            end_of(&next_text(&mut ended).await),
            UtteranceEnd::Cancelled(cause)
        );
    }
}

#[tokio::test]
async fn the_attached_app_cancels_with_each_allowed_cause() {
    for cause in ALLOWED {
        let world = world(Options::new(ScriptedInfer::new(vec![transcript(
            "a", "b", "c",
        )])))
        .await;
        let app = named(&world.bus, MAILO).await;
        let mut app_ended = ended_stream(&app).await;
        let (utterance, mut events) = start_utterance(&world, VoiceIntent::Ask).await;
        let path = utterance.inner().path().to_owned();
        utterance.route(&route_to(MAILO, 7)).await.expect("route");
        let theirs = UtteranceProxy::builder(&app)
            .path(path)
            .expect("path")
            .build()
            .await
            .expect("proxy");
        let _fd = theirs.attach().await.expect("attach");
        assert_eq!(events.next().await, Some(VoiceEvent::Opened));
        theirs.cancel(&cause_wire(cause)).await.expect("cancel");
        assert_eq!(
            end_of(&next_text(&mut app_ended).await),
            UtteranceEnd::Cancelled(cause)
        );
        assert_eq!(
            ended_of(&events.until_ended().await),
            UtteranceEnd::Cancelled(cause)
        );
    }
}

#[tokio::test]
async fn the_daemons_own_causes_and_bad_bodies_are_refused_and_the_mic_stays_open() {
    let world = world(Options::new(ScriptedInfer::new(vec![transcript(
        "a", "b", "c",
    )])))
    .await;
    let (utterance, mut events) = start_utterance(&world, VoiceIntent::Ask).await;
    assert_eq!(events.next().await, Some(VoiceEvent::Opened));
    let bodies = [
        cause_wire(CancelCause::Superseded),
        cause_wire(CancelCause::TooLong),
        "not json".to_owned(),
        "{\"vocab\":9,\"body\":\"shell\"}".to_owned(),
        "".to_owned(),
    ];
    for body in bodies {
        let error = utterance.cancel(&body).await.expect_err("refused");
        assert!(is_malformed(&error), "{body}: {error}");
    }
    assert_eq!(world.hand.open_now(), 1, "nothing was cancelled");
    utterance
        .cancel(&cause_wire(CancelCause::Escape))
        .await
        .expect("cancel");
    assert_eq!(
        ended_of(&events.until_ended().await),
        UtteranceEnd::Cancelled(CancelCause::Escape)
    );
}

#[tokio::test]
async fn a_stranger_may_not_cancel_whatever_the_cause() {
    let world = world(Options::new(ScriptedInfer::new(vec![transcript(
        "a", "b", "c",
    )])))
    .await;
    let stranger = world.bus.connect().await;
    let (utterance, mut events) = start_utterance(&world, VoiceIntent::Ask).await;
    assert_eq!(events.next().await, Some(VoiceEvent::Opened));
    let path = utterance.inner().path().to_owned();
    let theirs = UtteranceProxy::builder(&stranger)
        .path(path)
        .expect("path")
        .build()
        .await
        .expect("proxy");
    let error = theirs
        .cancel(&cause_wire(CancelCause::Shell))
        .await
        .expect_err("refused");
    assert_eq!(refusal(error), voice_wire::VoiceRefusal::NotAllowed);
    assert_eq!(world.hand.open_now(), 1);
}
