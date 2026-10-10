//! The daemon on a private bus: who may begin, what refuses, and one utterance from hold to text.

use crate::support::device::{OnOpen, ScriptedDevice, node};
use crate::support::infer::ScriptedInfer;
use crate::support::*;
use futures_util::FutureExt;
use futures_util::StreamExt;
use porter_core::capability::SpeechMode;
use porter_core::need::SpeechNeed;
use porter_core::{DataClass, Need, Tier};
use porter_fake::{Script, ScriptStep};
use porter_infer::{
    ClientFrame, InferEvent, InferRefusal, InferReply, InferRequest, LangPick, ModelError,
    Readiness, RequestKind, TranscribeMode,
};
use std::collections::BTreeSet;
use voice_wire::{
    CancelCause, HeardText, MicState, UtteranceEnd, VoiceEvent, VoiceFault, VoiceRefusal, VoiceUse,
};
use voiced::{DeviceError, NodeKind, VoiceProxy};

fn stt_need() -> Need {
    Need::Speech(SpeechNeed::new(BTreeSet::from([SpeechMode::Stt])))
}

fn audio_frames(frames: &[ClientFrame]) -> Vec<(u64, usize)> {
    frames
        .iter()
        .filter_map(|f| match f {
            ClientFrame::Audio(a) => Some((a.at, a.pcm.0.len() / 2)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn only_the_shell_begins_and_nothing_opens_for_anyone_else() {
    let world = world(Options::new(ScriptedInfer::new(vec![]))).await;
    let stranger = world.bus.connect().await;
    let app = named(&world.bus, MAILO).await;
    for connection in [&stranger, &app] {
        let voice = VoiceProxy::new(connection).await.expect("proxy");
        let refused = voice
            .begin(&begin(VoiceIntent::Ask))
            .await
            .expect_err("refused");
        assert_eq!(refusal(refused), VoiceRefusal::NotAllowed);
    }
    assert_eq!(world.hand.opened(), 0, "no capture was ever opened");
    assert!(world.seen.opens().is_empty(), "no inferd session either");
    let status = status_of(&world.voice.status().await.expect("status"));
    assert_eq!(status.mic, MicState::Closed);
}

#[tokio::test]
async fn consent_and_the_setting_gate_the_mic_before_it_opens() {
    for (usage, want) in [
        (VoiceUse::NeedsConsent, VoiceRefusal::NeedsConsent),
        (VoiceUse::Off, VoiceRefusal::Disabled),
    ] {
        let mut options = Options::new(ScriptedInfer::new(vec![]));
        options.usage = usage;
        let world = world(options).await;
        let refused = world
            .voice
            .begin(&begin(VoiceIntent::Ask))
            .await
            .expect_err("refused");
        assert_eq!(refusal(refused), want);
        assert_eq!(world.hand.opened(), 0);
    }
}

#[tokio::test]
async fn a_monitor_is_never_captured_and_no_source_means_no_mic() {
    let mut options = Options::new(ScriptedInfer::new(vec![]));
    options.device = ScriptedDevice::new(
        vec![
            node(1, NodeKind::Monitor, "Audio/Source"),
            node(3, NodeKind::Sink, "Audio/Sink"),
        ],
        OnOpen::Works,
    );
    let world = world(options).await;
    let refused = world
        .voice
        .begin(&begin(VoiceIntent::Ask))
        .await
        .expect_err("refused");
    assert_eq!(refusal(refused), VoiceRefusal::MicUnavailable);
    assert_eq!(world.hand.opened(), 0);
}

#[tokio::test]
async fn a_denied_microphone_ends_the_utterance_failed() {
    let mut options = Options::new(ScriptedInfer::new(vec![]));
    options.device = ScriptedDevice::new(
        vec![node(2, NodeKind::Source, "Audio/Source")],
        OnOpen::Fails(DeviceError::Denied),
    );
    let world = world(options).await;
    let mut ended = ended_stream(&world.shell).await;
    let (_, mut events) = start_utterance(&world, VoiceIntent::Ask).await;
    let seen = events.until_ended().await;
    assert_eq!(ended_of(&seen), UtteranceEnd::Failed(VoiceFault::MicDenied));
    assert!(!seen.contains(&VoiceEvent::Opened));
    assert_eq!(
        end_of(&next_text(&mut ended).await),
        UtteranceEnd::Failed(VoiceFault::MicDenied)
    );
    assert_eq!(world.hand.open_now(), 0);
}

#[tokio::test]
async fn hold_to_ask_hears_the_words_and_the_mic_closes_after_the_tail() {
    let world = world(Options::new(ScriptedInfer::new(vec![transcript(
        "hel",
        "hello",
        "hello world",
    )])))
    .await;
    let bystander = named(&world.bus, "org.example.Bystander").await;
    let mut ended = ended_stream(&world.shell).await;
    let mut bystander_ended = ended_stream(&bystander).await;
    let mut changes = status_stream(&bystander).await;
    let (utterance, mut events) = start_utterance(&world, VoiceIntent::Ask).await;

    assert_eq!(events.next().await, Some(VoiceEvent::Opened));
    assert_eq!(world.hand.open_now(), 1);
    assert_eq!(
        *world.hand.log.nodes.lock().expect("nodes"),
        [voiced::NodeId(2)],
        "the physical source, not the monitor"
    );
    assert_eq!(*world.hand.log.formats.lock().expect("formats"), [16_000]);
    let status = status_of(&world.voice.status().await.expect("status"));
    assert!(matches!(
        status.mic,
        MicState::Open {
            intent: VoiceIntent::Ask,
            ..
        }
    ));
    // The broadcast says only that the mic opened.
    let opened = status_of(&next_text(&mut changes).await);
    assert!(matches!(opened.mic, MicState::Open { .. }));

    let mut seen = feed_loud(&world, &mut events, 4).await;
    utterance.release().await.expect("release");
    for _ in 0..tail_frames() {
        world.hand.feed(quiet());
    }
    seen.extend(events.until_ended().await);

    let levels = seen
        .iter()
        .filter(|e| matches!(e, VoiceEvent::Level(_)))
        .count();
    assert_eq!(levels, 4, "one level per 32 ms frame, while listening only");
    let kinds: Vec<&str> = seen
        .iter()
        .filter_map(|e| match e {
            VoiceEvent::Partial(_) => Some("partial"),
            VoiceEvent::Committed(_) => Some("committed"),
            VoiceEvent::Ended(_) => Some("ended"),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, ["partial", "committed", "ended"]);
    let VoiceEvent::Ended(UtteranceEnd::Heard { text, served: by }) = seen.last().expect("last")
    else {
        panic!("not heard: {seen:?}");
    };
    assert_eq!((text.0.as_str(), by), ("hello world", &served()));
    assert_eq!(
        world.hand.open_now(),
        0,
        "the mic is closed once it is over"
    );

    // The signal carries the end without the text, to the Begin caller only.
    let signal = next_text(&mut ended).await;
    assert!(!signal.contains("hello"), "{signal}");
    assert!(matches!(end_of(&signal), UtteranceEnd::Heard { ref text, .. } if text.0.is_empty()));
    let closed = status_of(&next_text(&mut changes).await);
    assert_eq!(closed.mic, MicState::Closed);
    assert!(!format!("{closed:?}").contains("hello"));
    // A round trip orders the bystander's queue behind anything sent it earlier.
    VoiceProxy::new(&bystander)
        .await
        .expect("p")
        .status()
        .await
        .expect("status");
    assert!(
        bystander_ended.next().now_or_never().is_none(),
        "Ended is unicast"
    );

    // What inferd was asked, and what audio it got.
    assert_eq!(
        world.seen.opens(),
        [crate::support::infer::Opened {
            need: stt_need(),
            class: DataClass::Voice,
            tier: Tier::Balanced
        }]
    );
    let frames = world.seen.frames_of(0);
    let Some(ClientFrame::Request(InferRequest::Transcribe(begin))) = frames.first() else {
        panic!("no Transcribe first: {frames:?}");
    };
    assert_eq!(begin.mode, TranscribeMode::Streaming);
    assert_eq!(begin.lang, LangPick::Auto);
    assert_eq!(begin.rate.0, 16_000);
    assert_eq!(frames.last(), Some(&ClientFrame::EndOfAudio));
    let audio = audio_frames(&frames);
    let mut at = 0;
    for (start, len) in &audio {
        assert_eq!(*start, at, "in order, no gap");
        assert!(*len <= 16_000, "at most a second a frame");
        at += *len as u64;
    }
    assert_eq!(
        at,
        (4 + tail_frames() as u64) * 512,
        "every sample, tail included"
    );
}

#[tokio::test]
async fn a_cold_engine_buffers_the_audio_and_never_drops_it() {
    let (infer, open_gate) = ScriptedInfer::new(vec![transcript("a", "b", "c")]).cold();
    let world = world(Options::new(infer)).await;
    let (utterance, mut events) = start_utterance(&world, VoiceIntent::Ask).await;
    assert_eq!(events.next().await, Some(VoiceEvent::Opened));
    feed_loud(&world, &mut events, 3).await;
    assert!(
        world.seen.frames_of(0).is_empty(),
        "nothing reached a cold engine"
    );
    utterance.release().await.expect("release");
    open_gate.notify_one();
    for _ in 0..tail_frames() {
        world.hand.feed(quiet());
    }
    let seen = events.until_ended().await;
    assert!(matches!(ended_of(&seen), UtteranceEnd::Heard { .. }));
    let audio = audio_frames(&world.seen.frames_of(0));
    let total: usize = audio.iter().map(|(_, n)| n).sum();
    assert_eq!(
        total,
        (3 + tail_frames()) * 512,
        "all of it, in order, after the engine woke"
    );
    assert_eq!(audio[0].0, 0);
}

#[tokio::test]
async fn cancel_discards_everything_and_cancels_inferd() {
    let world = world(Options::new(ScriptedInfer::new(vec![transcript(
        "a", "b", "c",
    )])))
    .await;
    let mut ended = ended_stream(&world.shell).await;
    let (utterance, mut events) = start_utterance(&world, VoiceIntent::Ask).await;
    assert_eq!(events.next().await, Some(VoiceEvent::Opened));
    feed_loud(&world, &mut events, 2).await;
    utterance
        .cancel(&cause_wire(CancelCause::Shell))
        .await
        .expect("cancel");
    let seen = events.until_ended().await;
    assert_eq!(ended_of(&seen), UtteranceEnd::Cancelled(CancelCause::Shell));
    assert_eq!(
        end_of(&next_text(&mut ended).await),
        UtteranceEnd::Cancelled(CancelCause::Shell)
    );
    assert_eq!(world.hand.open_now(), 0);
    eventually(|| world.seen.frames_of(0).contains(&ClientFrame::Cancel)).await;
}

#[tokio::test]
async fn a_second_begin_supersedes_the_first() {
    let world = world(Options::new(ScriptedInfer::new(vec![
        transcript("a", "b", "c"),
        transcript("d", "e", "f"),
    ])))
    .await;
    let (_, mut first) = start_utterance(&world, VoiceIntent::Ask).await;
    assert_eq!(first.next().await, Some(VoiceEvent::Opened));
    let (_, mut second) = start_utterance(&world, VoiceIntent::Ask).await;
    assert_eq!(
        ended_of(&first.until_ended().await),
        UtteranceEnd::Cancelled(CancelCause::Superseded)
    );
    assert_eq!(second.next().await, Some(VoiceEvent::Opened));
    assert_eq!(world.hand.opened(), 2);
    assert_eq!(world.hand.open_now(), 1, "one mic at a time");
}

#[tokio::test]
async fn a_late_call_on_an_old_utterance_is_refused() {
    let world = world(Options::new(ScriptedInfer::new(vec![
        transcript("a", "b", "c"),
        transcript("d", "e", "f"),
    ])))
    .await;
    let (old, mut first) = start_utterance(&world, VoiceIntent::Ask).await;
    assert_eq!(first.next().await, Some(VoiceEvent::Opened));
    let (_, _second) = start_utterance(&world, VoiceIntent::Ask).await;
    first.until_ended().await;
    // The old object left the bus when the new one began; the call cannot reach the new mic.
    assert!(old.release().await.is_err());
    assert_eq!(world.hand.open_now(), 1);
}

#[tokio::test]
async fn the_engine_refusing_or_failing_ends_the_utterance_and_closes_the_mic() {
    let table = [
        (
            InferReply::Refused(InferRefusal::Denied),
            VoiceFault::Refused(InferRefusal::Denied),
        ),
        (
            InferReply::Failed(ModelError::Unreadable),
            VoiceFault::Engine(ModelError::Unreadable),
        ),
    ];
    for (reply, fault) in table {
        let world = world(Options::new(ScriptedInfer::new(vec![finish_with(reply)]))).await;
        let (utterance, mut events) = start_utterance(&world, VoiceIntent::Ask).await;
        assert_eq!(events.next().await, Some(VoiceEvent::Opened));
        utterance.release().await.expect("release");
        for _ in 0..tail_frames() {
            world.hand.feed(quiet());
        }
        assert_eq!(
            ended_of(&events.until_ended().await),
            UtteranceEnd::Failed(fault)
        );
        assert_eq!(world.hand.open_now(), 0);
    }
}

#[tokio::test]
async fn an_unreachable_inferd_fails_the_utterance_the_moment_it_is_known() {
    let infer = ScriptedInfer::new(vec![]).unreachable(porter_client::TransportError::Unreachable);
    let world = world(Options::new(infer)).await;
    let (_, mut events) = start_utterance(&world, VoiceIntent::Ask).await;
    assert_eq!(
        ended_of(&events.until_ended().await),
        UtteranceEnd::Failed(VoiceFault::Engine(ModelError::Unreachable))
    );
    assert_eq!(world.hand.open_now(), 0);
}

#[tokio::test]
async fn nothing_heard_when_the_transcript_is_empty() {
    let world = world(Options::new(ScriptedInfer::new(vec![transcript(
        "", "", "",
    )])))
    .await;
    let (utterance, mut events) = start_utterance(&world, VoiceIntent::Ask).await;
    assert_eq!(events.next().await, Some(VoiceEvent::Opened));
    utterance.release().await.expect("release");
    for _ in 0..tail_frames() {
        world.hand.feed(quiet());
    }
    assert_eq!(
        ended_of(&events.until_ended().await),
        UtteranceEnd::NothingHeard
    );
}

#[tokio::test]
async fn dictation_ends_by_itself_after_thirty_seconds_of_silence() {
    let world = world(Options::new(ScriptedInfer::new(vec![transcript(
        "", "", "",
    )])))
    .await;
    let (_, mut events) = start_utterance(&world, VoiceIntent::Dictate).await;
    assert_eq!(events.next().await, Some(VoiceEvent::Opened));
    // 30 s is 937.5 frames of 512 samples; quiet the whole time, no Release.
    for _ in 0..940 {
        world.hand.feed(quiet());
    }
    for _ in 0..tail_frames() {
        world.hand.feed(quiet());
    }
    assert_eq!(
        ended_of(&events.until_ended().await),
        UtteranceEnd::NothingHeard
    );
    assert_eq!(world.hand.open_now(), 0);
}

#[tokio::test]
async fn an_ask_keeps_listening_through_silence_until_release() {
    let world = world(Options::new(ScriptedInfer::new(vec![transcript(
        "", "", "",
    )])))
    .await;
    let (_, mut events) = start_utterance(&world, VoiceIntent::Ask).await;
    assert_eq!(events.next().await, Some(VoiceEvent::Opened));
    for _ in 0..940 {
        world.hand.feed(quiet());
    }
    // One level per frame: when the last has come, the daemon has taken them all.
    let mut levels = 0;
    while levels < 940 {
        levels += usize::from(matches!(events.next().await, Some(VoiceEvent::Level(_))));
    }
    let status = status_of(&world.voice.status().await.expect("status"));
    assert!(matches!(status.mic, MicState::Open { .. }));
    assert_eq!(world.hand.open_now(), 1);
}

#[tokio::test]
async fn the_capture_stream_dying_mid_hold_fails_the_utterance() {
    let world = world(Options::new(ScriptedInfer::new(vec![transcript(
        "a", "b", "c",
    )])))
    .await;
    let (_, mut events) = start_utterance(&world, VoiceIntent::Ask).await;
    assert_eq!(events.next().await, Some(VoiceEvent::Opened));
    world.hand.end_stream();
    assert_eq!(
        ended_of(&events.until_ended().await),
        UtteranceEnd::Failed(VoiceFault::MicUnavailable)
    );
    assert_eq!(world.hand.open_now(), 0);
}

#[tokio::test]
async fn prepare_is_the_shells_and_answers_a_readiness_slug() {
    let mut options = Options::new(ScriptedInfer::new(vec![]));
    options.warm = Readiness::Loading;
    let world = world(options).await;
    assert_eq!(world.voice.prepare().await.expect("prepare"), "loading");
    let stranger = world.bus.connect().await;
    let refused = VoiceProxy::new(&stranger)
        .await
        .expect("proxy")
        .prepare()
        .await
        .expect_err("refused");
    assert_eq!(refusal(refused), VoiceRefusal::NotAllowed);
    assert_eq!(world.hand.opened(), 0, "prepare never opens the mic");
}

#[tokio::test]
async fn a_malformed_or_foreign_body_is_refused_before_anything_opens() {
    let world = world(Options::new(ScriptedInfer::new(vec![]))).await;
    for body in [
        "not json",
        "{\"vocab\":9,\"body\":{}}",
        "{\"intent\":\"ask\"}",
    ] {
        let error = world.voice.begin(body).await.expect_err("refused");
        assert!(
            matches!(&error, zbus::Error::MethodError(name, _, _)
            if name.as_str() == "org.quire.Voice1.Error.Malformed"),
            "{error}"
        );
    }
    assert_eq!(world.hand.opened(), 0);
}

#[tokio::test]
async fn a_second_voiced_on_the_same_bus_is_refused() {
    let world = world(Options::new(ScriptedInfer::new(vec![]))).await;
    let (device, _) = ScriptedDevice::with_a_microphone();
    let seams = voiced::Seams {
        device,
        transport: ScriptedInfer::new(vec![]),
        warm: voiced::FixedWarm(Readiness::Ready),
        usage: voiced::FixedUse(VoiceUse::On),
        proc_root: std::path::PathBuf::from("/nonexistent-proc"),
    };
    let second = voiced::start(world.bus.connect().await, config(), seams).await;
    assert!(second.is_err(), "the name is taken");
}

#[tokio::test]
async fn an_utterance_leaves_nothing_on_disk() {
    let world = world(Options::new(ScriptedInfer::new(vec![transcript(
        "a", "b", "c",
    )])))
    .await;
    let before = files_under(world.home.path());
    let (utterance, mut events) = start_utterance(&world, VoiceIntent::Ask).await;
    assert_eq!(events.next().await, Some(VoiceEvent::Opened));
    feed_loud(&world, &mut events, 4).await;
    utterance.release().await.expect("release");
    for _ in 0..tail_frames() {
        world.hand.feed(quiet());
    }
    events.until_ended().await;
    assert_eq!(
        files_under(world.home.path()),
        before,
        "no audio or text was written"
    );
}

fn files_under(dir: &std::path::Path) -> Vec<(std::path::PathBuf, u64)> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.metadata() {
                Ok(m) if m.is_dir() => pending.push(path),
                Ok(m) if m.is_file() => found.push((path, m.len())),
                _ => {}
            }
        }
    }
    found.sort();
    found
}

#[tokio::test]
async fn a_request_inferd_cannot_serve_ends_the_utterance_refused() {
    // The scripted session has a script for speaking only: a transcription is `Unsupported`.
    let speak_only = vec![Script {
        kind: RequestKind::Speak,
        steps: vec![ScriptStep::Emit(InferEvent::Finished(
            InferReply::Cancelled,
        ))],
    }];
    let world = world(Options::new(ScriptedInfer::new(vec![speak_only]))).await;
    let (_, mut events) = start_utterance(&world, VoiceIntent::Ask).await;
    assert_eq!(
        ended_of(&events.until_ended().await),
        UtteranceEnd::Failed(VoiceFault::Refused(InferRefusal::Unsupported))
    );
    assert_eq!(world.hand.open_now(), 0);
}

#[tokio::test]
async fn dictation_runs_in_process_over_the_device_seam_with_no_bus() {
    let (device, hand) = ScriptedDevice::with_a_microphone();
    let infer = ScriptedInfer::new(vec![transcript("hel", "hello", "hello world")]);
    let seen = infer.seen.clone();
    let task = tokio::spawn(async move { voiced::dictate(&device, &infer, None).await });
    while hand.feed(loud()) == 0 {
        tokio::task::yield_now().await;
    }
    for _ in 0..3 {
        hand.feed(loud());
    }
    hand.end_stream();
    let dictated = task.await.expect("joined").expect("dictated");
    assert_eq!(dictated.text, HeardText("hello world".to_owned()));
    assert_eq!(seen.opens().len(), 1, "one inferd session");
    assert_eq!(hand.open_now(), 0, "the mic is closed again");
    let sent = audio_frames(&seen.frames_of(0));
    assert_eq!(sent, [(0, 512), (512, 512), (1024, 512), (1536, 512)]);
}
