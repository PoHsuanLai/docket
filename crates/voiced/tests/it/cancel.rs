//! `Utterance.Cancel(cause)`: the caller's cause reaches `Ended` unchanged; the daemon's own
//! causes are refused. Private bus, fake device, no clock.

use crate::support::infer::ScriptedInfer;
use crate::support::*;
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

#[tokio::test]
async fn a_hold_that_is_never_released_ends_as_too_long_and_closes_the_mic() {
    let world = world_with(
        Options::new(ScriptedInfer::new(vec![transcript("a", "b", "c")])),
        config_with_hold(300),
    )
    .await;
    let (_utterance, mut events) = start_utterance(&world, VoiceIntent::Ask).await;
    assert_eq!(events.next().await, Some(VoiceEvent::Opened));
    assert_eq!(world.hand.open_now(), 1);
    assert_eq!(
        ended_of(&events.until_ended().await),
        UtteranceEnd::Cancelled(CancelCause::TooLong)
    );
    assert_eq!(world.hand.open_now(), 0, "the mic closed with the hold");
}

#[tokio::test]
async fn a_cold_engine_past_the_buffer_ends_the_utterance_as_too_long() {
    let (infer, _gate) = ScriptedInfer::new(vec![transcript("a", "b", "c")]).cold();
    let world = world(Options::new(infer)).await;
    let (_utterance, mut events) = start_utterance(&world, VoiceIntent::Ask).await;
    assert_eq!(events.next().await, Some(VoiceEvent::Opened));
    // Eleven seconds of audio against a ten second buffer.
    for _ in 0..(11 * 16_000 / 512) {
        world.hand.feed(loud());
    }
    assert_eq!(
        ended_of(&events.until_ended().await),
        UtteranceEnd::Cancelled(CancelCause::TooLong)
    );
    assert_eq!(world.hand.open_now(), 0);
}

#[tokio::test]
async fn a_begin_caller_that_leaves_the_bus_closes_the_mic() {
    let world = world_with(
        Options::new(ScriptedInfer::new(vec![transcript("a", "b", "c")])),
        config_with_hold(600_000),
    )
    .await;
    let second = named(&world.bus, SECOND_SHELL).await;
    let voice = voiced::VoiceProxy::new(&second).await.expect("proxy");
    let (_path, fd) = voice.begin(&begin(VoiceIntent::Ask)).await.expect("begin");
    let mut events = Events::new(fd);
    assert_eq!(events.next().await, Some(VoiceEvent::Opened));
    assert_eq!(world.hand.open_now(), 1);
    drop(voice);
    drop(second);
    assert_eq!(
        ended_of(&events.until_ended().await),
        UtteranceEnd::Cancelled(CancelCause::Shell)
    );
    assert_eq!(world.hand.open_now(), 0, "nobody was left to release it");
}
