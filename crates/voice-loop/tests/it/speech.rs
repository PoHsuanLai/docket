//! The speech table, barge-in order, the sentencer and the buffer.

use docket_core::VoiceIntent;
use porter_core::DataClass;
use porter_core::capability::LanguageTag;
use voice_loop::*;
use voice_wire::{SpeakWire, SpeechEnd, SpokenText, VoiceUse};

fn wire(text: &str) -> SpeakWire {
    SpeakWire {
        utterance: None,
        text: SpokenText(text.into()),
        class: DataClass::Mail,
        lang: LanguageTag::parse("en").expect("l"),
    }
}
fn s(t: &str) -> Sentence {
    Sentence(t.into())
}

use SpeechEffect as E;
use SpeechEvent as V;
use SpeechPhase as P;

fn state(phase: SpeechPhase, queue: &[&str], mic: MicNow) -> SpeechState {
    SpeechState {
        phase,
        queue: queue.iter().map(|t| s(t)).collect(),
        pending: None,
        mic,
        class: Some(DataClass::Mail),
    }
}

#[test]
fn speech_table() {
    let long_text = "First sentence here. Second sentence here.";
    type Row = (
        &'static str,
        SpeechState,
        SpeechEvent,
        SpeechState,
        Vec<SpeechEffect>,
    );
    let rows: Vec<Row> = vec![
        (
            "speak while quiet starts synthesis",
            SpeechState::quiet(),
            V::Speak(wire(long_text)),
            SpeechState {
                phase: P::Synth,
                queue: vec![s("Second sentence here.")],
                pending: None,
                mic: MicNow::Closed,
                class: Some(DataClass::Mail),
            },
            vec![
                E::InferOpen(DataClass::Mail),
                E::SpeakSentence(s("First sentence here.")),
            ],
        ),
        (
            "speak while the mic is open waits",
            SpeechState {
                mic: MicNow::Open,
                ..SpeechState::quiet()
            },
            V::Speak(wire("Hello there friend.")),
            SpeechState {
                phase: P::Quiet,
                queue: vec![],
                pending: Some(wire("Hello there friend.")),
                mic: MicNow::Open,
                class: None,
            },
            vec![],
        ),
        (
            "the mic closing starts the pending speech",
            SpeechState {
                pending: Some(wire("Hello there friend.")),
                mic: MicNow::Open,
                ..SpeechState::quiet()
            },
            V::Mic(MicNow::Closed),
            SpeechState {
                phase: P::Synth,
                queue: vec![],
                pending: None,
                mic: MicNow::Closed,
                class: Some(DataClass::Mail),
            },
            vec![
                E::InferOpen(DataClass::Mail),
                E::SpeakSentence(s("Hello there friend.")),
            ],
        ),
        (
            "audio makes it play",
            state(P::Synth, &[], MicNow::Closed),
            V::Chunk,
            state(P::Playing, &[], MicNow::Closed),
            vec![E::Play],
        ),
        (
            "a finished sentence starts the next",
            state(P::Playing, &["Next one here."], MicNow::Closed),
            V::SentenceDone,
            state(P::Synth, &[], MicNow::Closed),
            vec![E::SpeakSentence(s("Next one here."))],
        ),
        (
            "a drained queue is done",
            state(P::Playing, &[], MicNow::Closed),
            V::Drained,
            SpeechState {
                class: None,
                ..state(P::Quiet, &[], MicNow::Closed)
            },
            vec![E::Finished(SpeechEnd::Done)],
        ),
        (
            "drained with sentences left keeps playing",
            state(P::Playing, &["more"], MicNow::Closed),
            V::Drained,
            state(P::Playing, &["more"], MicNow::Closed),
            vec![],
        ),
        (
            "hush stops",
            state(P::Playing, &["more"], MicNow::Closed),
            V::Hush,
            state(P::Stopping, &[], MicNow::Closed),
            vec![
                E::StopSynth,
                E::Fade { ms: FADE_MS },
                E::Finished(SpeechEnd::Hushed),
            ],
        ),
        (
            "an utterance barges in",
            state(P::Synth, &[], MicNow::Closed),
            V::Begin,
            state(P::Stopping, &[], MicNow::Closed),
            vec![
                E::StopSynth,
                E::Fade { ms: FADE_MS },
                E::Finished(SpeechEnd::BargedIn),
            ],
        ),
        (
            "stopping drains to quiet",
            state(P::Stopping, &[], MicNow::Open),
            V::Drained,
            SpeechState {
                class: None,
                ..state(P::Quiet, &[], MicNow::Open)
            },
            vec![],
        ),
        (
            "speech while playing extends the queue",
            state(P::Playing, &[], MicNow::Closed),
            V::Speak(wire("Another long sentence follows.")),
            state(
                P::Playing,
                &["Another long sentence follows."],
                MicNow::Closed,
            ),
            vec![],
        ),
        (
            "begin while quiet does nothing",
            SpeechState::quiet(),
            V::Begin,
            SpeechState::quiet(),
            vec![],
        ),
        (
            "an empty text is done at once",
            SpeechState::quiet(),
            V::Speak(wire("  ")),
            SpeechState::quiet(),
            vec![E::Finished(SpeechEnd::Done)],
        ),
    ];
    for (name, from, event, to, effects) in rows {
        assert_eq!(speech_step(from, event), (to, effects), "case: {name}");
    }
}

#[test]
fn barge_in_stops_before_mic_opens() {
    let speaking = state(P::Playing, &["x"], MicNow::Closed);
    let begin = UtteranceEvent::Begin {
        enabled: VoiceUse::On,
        intent: VoiceIntent::Ask,
    };
    let (speech, utterance, steps) = begin_utterance(speaking, UtteranceState::Idle, begin);
    let at = |step: Step| steps.iter().position(|s| *s == step).expect("step present");
    assert!(at(Step::Speech(E::StopSynth)) < at(Step::Utterance(UtteranceEffect::OpenMic)));
    assert_eq!(utterance, UtteranceState::Opening);
    assert_eq!(speech.phase, P::Stopping);
    assert_eq!(speech.mic, MicNow::Open);
}

#[test]
fn speak_is_refused_while_the_mic_is_open_then_runs() {
    let (waiting, effects) = speech_step(
        SpeechState {
            mic: MicNow::Open,
            ..SpeechState::quiet()
        },
        V::Speak(wire("Hello there friend.")),
    );
    assert!(effects.is_empty() && waiting.pending.is_some());
    let (running, effects) = speech_step(waiting, V::Mic(MicNow::Closed));
    assert_eq!(running.phase, P::Synth);
    assert!(effects.contains(&E::SpeakSentence(s("Hello there friend."))));
}

#[test]
fn sentencer_table() {
    let long_word = "a".repeat(700);
    let long_spaced = "word ".repeat(100);
    let cases: Vec<(&str, &str, Vec<&str>)> = vec![
        (
            "english sentences",
            "It is done. Did it work? Yes it did work well!",
            vec!["It is done. Did it work?", "Yes it did work well!"],
        ),
        (
            "a decimal stays whole",
            "The total is 3.5 euros today.",
            vec!["The total is 3.5 euros today."],
        ),
        (
            "newlines split",
            "First line is here\nSecond line is here",
            vec!["First line is here", "Second line is here"],
        ),
        (
            "chinese punctuation",
            "你好，今天天气很好。我们去公园玩吧！",
            vec!["你好，今天天气很好。我们去公园玩吧！"],
        ),
        (
            "a short piece joins its neighbour",
            "Ok. This is a longer sentence.",
            vec!["Ok. This is a longer sentence."],
        ),
        ("empty", "", vec![]),
        ("blank lines", "\n\n  \n", vec![]),
    ];
    for (name, text, want) in cases {
        let got: Vec<String> = sentences(text).into_iter().map(|x| x.0).collect();
        assert_eq!(got, want, "case: {name}");
    }
    let capped = sentences(&long_word);
    assert_eq!(
        capped
            .iter()
            .map(|x| x.0.chars().count())
            .collect::<Vec<_>>(),
        [300, 300, 100]
    );
    let spaced = sentences(&long_spaced);
    assert!(
        spaced.len() >= 2
            && spaced
                .iter()
                .all(|x| x.0.chars().count() <= MAX_SENTENCE_CHARS),
        "{:?}",
        spaced.len()
    );
    let zh = sentences(&"好".repeat(10).repeat(40));
    assert!(zh.iter().all(|x| x.0.chars().count() <= 300));
}

#[test]
fn zh_sentences_split_and_join_without_spaces() {
    let got: Vec<String> =
        sentences("我们今天去公园玩，天气非常好呀。明天我们再去看一场很好看的电影吧！好")
            .into_iter()
            .map(|x| x.0)
            .collect();
    assert_eq!(
        got,
        [
            "我们今天去公园玩，天气非常好呀。",
            "明天我们再去看一场很好看的电影吧！好"
        ]
    );
}

#[test]
fn buffer_caps_at_10s() {
    let mut buffer = PcmBuffer::new();
    assert_eq!(buffer.push(&vec![1; BUFFER_SAMPLES]), PushOutcome::Kept);
    assert_eq!(buffer.len(), BUFFER_SAMPLES);
    assert_eq!(
        buffer.push(&[2, 3]),
        PushOutcome::DroppedOldest { samples: 2 }
    );
    assert_eq!(buffer.len(), BUFFER_SAMPLES);
    let all = buffer.take();
    assert_eq!(all.len(), BUFFER_SAMPLES);
    assert_eq!(&all[BUFFER_SAMPLES - 2..], &[2, 3]);
    assert_eq!(all[0], 1);
    assert!(buffer.is_empty());
    let mut small = PcmBuffer::new();
    assert_eq!(
        small.push(&vec![7; BUFFER_SAMPLES + 5]),
        PushOutcome::DroppedOldest { samples: 5 }
    );
}

#[test]
fn buffers_zeroized_on_end() {
    let mut buffer = PcmBuffer::new();
    buffer.push(&[5; 1000]);
    assert_eq!(buffer.nonzero_slots(), 1000);
    buffer.wipe();
    assert_eq!(buffer.nonzero_slots(), 0);
    assert!(buffer.is_empty());
    buffer.push(&[9; 10]);
    let _ = buffer.take();
    assert_eq!(buffer.nonzero_slots(), 0, "take wipes");
    assert!(!format!("{buffer:?}").contains('9'));
}
