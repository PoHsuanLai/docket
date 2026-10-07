//! The routed app: what it may do with an utterance, and what the shell alone may.

use crate::support::infer::ScriptedInfer;
use crate::support::*;
use futures_util::FutureExt;
use futures_util::StreamExt;
use voice_wire::{CancelCause, SpeechEnd, UtteranceEnd, VoiceEvent, VoiceRefusal};
use voiced::{UtteranceProxy, VoiceProxy};

async fn as_app(
    world: &World,
    app: &zbus::Connection,
    path: &zbus::zvariant::OwnedObjectPath,
) -> UtteranceProxy<'static> {
    let _ = world;
    UtteranceProxy::builder(app)
        .path(path.clone())
        .expect("path")
        .build()
        .await
        .expect("proxy")
}

#[tokio::test]
async fn the_routed_app_attaches_once_and_is_replayed_what_was_heard() {
    let world = world(Options::new(ScriptedInfer::new(vec![transcript(
        "hel",
        "hello",
        "hello world",
    )])))
    .await;
    let app = named(&world.bus, MAILO).await;
    let stranger = world.bus.connect().await;
    let mut shell_ended = ended_stream(&world.shell).await;
    let mut app_ended = ended_stream(&app).await;
    let mut stranger_ended = ended_stream(&stranger).await;
    let (utterance, mut events) = start_utterance(&world, VoiceIntent::Dictate).await;
    let path = utterance.inner().path().to_owned();
    let theirs = as_app(&world, &app, &path.clone().into()).await;

    // Not routed yet: nobody may attach.
    assert_eq!(
        refusal(theirs.attach().await.expect_err("refused")),
        VoiceRefusal::NotAllowed
    );
    // Only the Begin caller routes.
    assert_eq!(
        refusal(
            theirs
                .route(&route_to(MAILO, 1))
                .await
                .expect_err("refused")
        ),
        VoiceRefusal::NotAllowed
    );
    utterance.route(&route_to(MAILO, 1)).await.expect("route");

    assert_eq!(events.next().await, Some(VoiceEvent::Opened));
    feed_loud(&world, &mut events, 4).await;
    events
        .until(|e| matches!(e, VoiceEvent::Committed(_)))
        .await;

    // A stranger is not the routed app; the app is.
    let strangers = as_app(&world, &stranger, &path.clone().into()).await;
    assert_eq!(
        refusal(strangers.attach().await.expect_err("refused")),
        VoiceRefusal::NotAllowed
    );
    let mut attached = Events::new(theirs.attach().await.expect("attach"));
    assert_eq!(
        refusal(theirs.attach().await.expect_err("busy")),
        VoiceRefusal::Busy
    );
    // It starts with the committed segments (and the mic state), then follows.
    let replay = attached
        .until(|e| matches!(e, VoiceEvent::Committed(_)))
        .await;
    assert_eq!(replay.first(), Some(&VoiceEvent::Opened));
    assert!(matches!(replay.last(), Some(VoiceEvent::Committed(c)) if c.text.0 == "hello"));

    // Only the Begin caller ends the hold.
    assert_eq!(
        refusal(theirs.release().await.expect_err("refused")),
        VoiceRefusal::NotAllowed
    );
    utterance.release().await.expect("release");
    for _ in 0..tail_frames() {
        world.hand.feed(quiet());
    }
    let end = attached.until_ended().await;
    assert!(
        matches!(ended_of(&end), UtteranceEnd::Heard { ref text, .. } if text.0 == "hello world")
    );
    events.until_ended().await;

    // Both got the unicast signal, without the text; the stranger got nothing.
    for stream in [&mut shell_ended, &mut app_ended] {
        let signal = next_text(stream).await;
        assert!(!signal.contains("hello"), "{signal}");
        assert!(matches!(end_of(&signal), UtteranceEnd::Heard { .. }));
    }
    VoiceProxy::new(&stranger)
        .await
        .expect("p")
        .status()
        .await
        .expect("status");
    assert!(stranger_ended.next().now_or_never().is_none());
}

#[tokio::test]
async fn the_attached_app_may_cancel_and_may_speak_to_its_utterance() {
    let world = world(Options::new(ScriptedInfer::new(vec![transcript(
        "a", "b", "c",
    )])))
    .await;
    let app = named(&world.bus, MAILO).await;
    let mut app_ended = ended_stream(&app).await;
    let mut finished = finished_stream(&app).await;
    let (utterance, mut events) = start_utterance(&world, VoiceIntent::Ask).await;
    let path = utterance.inner().path().to_owned();
    utterance.route(&route_to(MAILO, 7)).await.expect("route");
    let theirs = as_app(&world, &app, &path.into()).await;
    let _fd = theirs.attach().await.expect("attach");
    assert_eq!(events.next().await, Some(VoiceEvent::Opened));

    // Its own utterance: it may speak, queued behind the open mic.
    let voice = VoiceProxy::new(&app).await.expect("proxy");
    voice
        .speak(&speak_wire(FIRST, Some("u-1")))
        .await
        .expect("speak to u-1");
    assert_eq!(
        refusal(
            voice
                .speak(&speak_wire(FIRST, Some("u-2")))
                .await
                .expect_err("refused")
        ),
        VoiceRefusal::NotAllowed,
        "not an utterance it is attached to"
    );

    theirs
        .cancel(&cause_wire(CancelCause::FocusLost))
        .await
        .expect("cancel");
    assert_eq!(
        end_of(&next_text(&mut app_ended).await),
        UtteranceEnd::Cancelled(CancelCause::FocusLost)
    );
    assert_eq!(world.hand.open_now(), 0);
    // The speech it asked for goes unspoken here (no synthesis scripted): it still finishes.
    let end = speech_end_of(&next_text(&mut finished).await);
    assert!(
        matches!(end, SpeechEnd::Failed(_) | SpeechEnd::Done),
        "{end:?}"
    );
}
