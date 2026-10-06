//! Speech output on a private bus: sentence by sentence, half-duplex, barge-in and hush.

mod support;

use futures_util::{FutureExt, StreamExt};
use porter_core::capability::SpeechMode;
use porter_core::need::SpeechNeed;
use porter_core::{DataClass, Need};
use porter_infer::{ClientFrame, InferRequest};
use std::collections::BTreeSet;
use support::infer::{Opened, ScriptedInfer};
use support::*;
use voice_wire::{SpeechEnd, VoiceEvent, VoiceRefusal};
use voiced::{SpeechProxy, VoiceProxy};

fn tts_need() -> Need {
    Need::Speech(SpeechNeed {
        modes: BTreeSet::from([SpeechMode::Tts]),
    })
}

fn spoken_texts(frames: &[ClientFrame]) -> Vec<String> {
    frames
        .iter()
        .filter_map(|f| match f {
            ClientFrame::Request(InferRequest::Speak(request)) => Some(request.text.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn an_answer_is_spoken_sentence_by_sentence_and_finishes_done() {
    let infer = ScriptedInfer::new(vec![vec![sentence_script(480), sentence_script(240)]]);
    let world = world(Options::new(infer)).await;
    let bystander = world.bus.connect().await;
    let mut finished = finished_stream(&world.shell).await;
    let mut other = finished_stream(&bystander).await;
    let path = world
        .voice
        .speak(&speak_wire(&format!("{FIRST} {SECOND}"), None))
        .await
        .expect("speak");
    assert!(path.as_str().starts_with("/org/quire/Voice1/speech/"));
    assert_eq!(
        speech_end_of(&next_text(&mut finished).await),
        SpeechEnd::Done
    );

    assert_eq!(
        world.seen.opens(),
        [Opened {
            need: tts_need(),
            class: DataClass::Mail,
            tier: porter_core::Tier::Balanced
        }],
        "the text's own class, one session for the answer"
    );
    assert_eq!(
        spoken_texts(&world.seen.frames_of(0)),
        [FIRST.to_owned(), SECOND.to_owned()]
    );
    eventually(|| world.hand.log.played.lock().expect("played").len() == 2).await;
    let played = world.hand.log.played.lock().expect("played").clone();
    assert_eq!(
        played.iter().map(|(rate, _)| *rate).collect::<Vec<_>>(),
        [24_000, 24_000]
    );
    assert_eq!(
        played.iter().map(|(_, s)| s.len()).collect::<Vec<_>>(),
        [480, 240]
    );
    VoiceProxy::new(&bystander)
        .await
        .expect("p")
        .status()
        .await
        .expect("status");
    assert!(
        other.next().now_or_never().is_none(),
        "Finished goes to the requester only"
    );
}

#[tokio::test]
async fn only_the_shell_or_the_attached_app_may_speak() {
    let world = world(Options::new(ScriptedInfer::new(vec![]))).await;
    let stranger = world.bus.connect().await;
    let app = named(&world.bus, MAILO).await;
    for (connection, utterance) in [(&stranger, None), (&app, None), (&app, Some("u-1"))] {
        let refused = VoiceProxy::new(connection)
            .await
            .expect("proxy")
            .speak(&speak_wire(FIRST, utterance))
            .await
            .expect_err("refused");
        assert_eq!(refusal(refused), VoiceRefusal::NotAllowed);
    }
    assert!(world.seen.opens().is_empty());
}

#[tokio::test]
async fn speech_waits_for_the_mic_and_starts_when_the_utterance_ends() {
    let infer = ScriptedInfer::new(vec![transcript("a", "b", "c"), vec![sentence_script(480)]]);
    let world = world(Options::new(infer)).await;
    let mut finished = finished_stream(&world.shell).await;
    let (utterance, mut events) = start_utterance(&world, VoiceIntent::Ask).await;
    assert_eq!(events.next().await, Some(VoiceEvent::Opened));
    world
        .voice
        .speak(&speak_wire(FIRST, None))
        .await
        .expect("queued");
    let status = status_of(&world.voice.status().await.expect("status"));
    assert_eq!(
        status.speaking,
        voice_wire::Speaking::Quiet,
        "nothing plays while the mic is open"
    );
    assert_eq!(world.seen.opens().len(), 1, "no synthesis session yet");
    utterance
        .cancel(&cause_wire(voice_wire::CancelCause::Shell))
        .await
        .expect("cancel");
    assert_eq!(
        speech_end_of(&next_text(&mut finished).await),
        SpeechEnd::Done
    );
    assert_eq!(world.seen.opens().len(), 2);
    assert_eq!(spoken_texts(&world.seen.frames_of(1)), [FIRST.to_owned()]);
}

#[tokio::test]
async fn a_begin_barges_in_on_speech_first() {
    let infer = ScriptedInfer::new(vec![vec![endless_script()], transcript("a", "b", "c")]);
    let world = world(Options::new(infer)).await;
    let mut finished = finished_stream(&world.shell).await;
    world
        .voice
        .speak(&speak_wire(FIRST, None))
        .await
        .expect("speak");
    eventually(|| !world.hand.log.played.lock().expect("played").is_empty()).await;
    let (_, mut events) = start_utterance(&world, VoiceIntent::Ask).await;
    assert_eq!(
        speech_end_of(&next_text(&mut finished).await),
        SpeechEnd::BargedIn
    );
    assert_eq!(events.next().await, Some(VoiceEvent::Opened));
    eventually(|| *world.hand.log.faded.lock().expect("faded") == [voice_loop::FADE_MS]).await;
    assert!(
        world.seen.frames_of(0).contains(&ClientFrame::Cancel),
        "synthesis was cancelled"
    );
    assert_eq!(world.hand.open_now(), 1);
}

#[tokio::test]
async fn hush_stops_speech_for_the_shell_alone() {
    let world = world(Options::new(ScriptedInfer::new(vec![vec![
        endless_script(),
    ]])))
    .await;
    let mut finished = finished_stream(&world.shell).await;
    world
        .voice
        .speak(&speak_wire(FIRST, None))
        .await
        .expect("speak");
    let stranger = world.bus.connect().await;
    let refused = VoiceProxy::new(&stranger)
        .await
        .expect("proxy")
        .hush()
        .await
        .expect_err("refused");
    assert_eq!(refusal(refused), VoiceRefusal::NotAllowed);
    world.voice.hush().await.expect("hush");
    assert_eq!(
        speech_end_of(&next_text(&mut finished).await),
        SpeechEnd::Hushed
    );
    eventually(|| !world.hand.log.faded.lock().expect("faded").is_empty()).await;
}

#[tokio::test]
async fn the_requester_may_stop_its_own_speech_and_a_stranger_may_not() {
    let world = world(Options::new(ScriptedInfer::new(vec![vec![
        endless_script(),
    ]])))
    .await;
    let mut finished = finished_stream(&world.shell).await;
    let path = world
        .voice
        .speak(&speak_wire(FIRST, None))
        .await
        .expect("speak");
    let stranger = world.bus.connect().await;
    let theirs = SpeechProxy::builder(&stranger)
        .path(path.clone())
        .expect("path")
        .build()
        .await
        .expect("proxy");
    let refused = theirs.stop().await.expect_err("refused");
    assert_eq!(refusal(refused), VoiceRefusal::NotAllowed);
    let ours = SpeechProxy::builder(&world.shell)
        .path(path)
        .expect("path")
        .build()
        .await
        .expect("proxy");
    ours.stop().await.expect("stop");
    assert_eq!(
        speech_end_of(&next_text(&mut finished).await),
        SpeechEnd::Hushed
    );
}

#[tokio::test]
async fn synthesis_refused_ends_the_speech_failed() {
    // No script for speaking: the session answers Unsupported.
    let world = world(Options::new(ScriptedInfer::new(vec![vec![]]))).await;
    let mut finished = finished_stream(&world.shell).await;
    world
        .voice
        .speak(&speak_wire(FIRST, None))
        .await
        .expect("speak");
    let end = speech_end_of(&next_text(&mut finished).await);
    assert!(
        matches!(end, SpeechEnd::Failed(voice_wire::VoiceFault::Refused(_))),
        "{end:?}"
    );
    let status = status_of(&world.voice.status().await.expect("status"));
    assert_eq!(status.speaking, voice_wire::Speaking::Quiet);
}
