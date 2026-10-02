//! Speech output with barge-in (§4.3). Half-duplex by construction: speech never plays while the
//! mic is open, so no echo cancellation is needed. A `Speak` while the mic is open waits (depth
//! one) and starts when the utterance ends; any new utterance stops speech first.

use crate::sentencer::{Sentence, sentences};
use crate::utterance::MicNow;
use porter_core::DataClass;
use serde::{Deserialize, Serialize};
use voice_wire::{SpeakWire, SpeechEnd};

/// The fade-out on a stop, in milliseconds.
pub const FADE_MS: u32 = 40;

/// Where speech is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeechPhase {
    /// Silent.
    Quiet,
    /// Waiting for the first audio of a sentence.
    Synth,
    /// Playing.
    Playing,
    /// Fading out and draining.
    Stopping,
}

/// The machine's state: the phase, the sentences still to speak, a request waiting for the mic,
/// and whether the mic is open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeechState {
    /// The phase.
    pub phase: SpeechPhase,
    /// Sentences not yet sent to synthesis.
    pub queue: Vec<Sentence>,
    /// A request that arrived while the mic was open or speech was stopping (depth one).
    pub pending: Option<SpeakWire>,
    /// The mic, as the utterance machine reports it.
    pub mic: MicNow,
    /// The data class of what is being spoken.
    pub class: Option<DataClass>,
}

impl SpeechState {
    /// Silent, mic closed.
    pub fn quiet() -> Self {
        Self {
            phase: SpeechPhase::Quiet,
            queue: Vec::new(),
            pending: None,
            mic: MicNow::Closed,
            class: None,
        }
    }
}

/// What happens to speech.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum SpeechEvent {
    /// `Voice1.Speak`.
    Speak(SpeakWire),
    /// Synthesised audio arrived.
    Chunk,
    /// Synthesis of a sentence finished.
    SentenceDone,
    /// Playback drained.
    Drained,
    /// `Voice1.Hush` or `Speech.Stop`.
    Hush,
    /// An utterance began (any utterance).
    Begin,
    /// The mic opened or closed.
    Mic(MicNow),
}

/// What the daemon does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum SpeechEffect {
    /// Open the inferd speech session for this class.
    InferOpen(DataClass),
    /// Synthesise this sentence.
    SpeakSentence(Sentence),
    /// Write the chunk to the playback stream.
    Play,
    /// Stop synthesis (`Flow::Stop`) and cancel the request.
    StopSynth,
    /// Fade out over this many milliseconds.
    Fade {
        /// How long.
        ms: u32,
    },
    /// Tell the requester how it ended.
    Finished(SpeechEnd),
}

use SpeechEffect as E;
use SpeechEvent as V;
use SpeechPhase as P;

fn start(mut state: SpeechState, wire: &SpeakWire) -> (SpeechState, Vec<SpeechEffect>) {
    let mut queue = sentences(wire.text.0.as_str());
    if queue.is_empty() {
        return (
            SpeechState {
                phase: P::Quiet,
                queue,
                pending: None,
                class: None,
                ..state
            },
            vec![E::Finished(SpeechEnd::Done)],
        );
    }
    let first = queue.remove(0);
    state.phase = P::Synth;
    state.queue = queue;
    state.pending = None;
    state.class = Some(wire.class);
    (
        state,
        vec![E::InferOpen(wire.class), E::SpeakSentence(first)],
    )
}

fn stop(mut state: SpeechState, end: SpeechEnd) -> (SpeechState, Vec<SpeechEffect>) {
    state.phase = P::Stopping;
    state.queue.clear();
    (
        state,
        vec![E::StopSynth, E::Fade { ms: FADE_MS }, E::Finished(end)],
    )
}

fn next_sentence(mut state: SpeechState) -> (SpeechState, Vec<SpeechEffect>) {
    if state.queue.is_empty() {
        (state, vec![])
    } else {
        let next = state.queue.remove(0);
        state.phase = P::Synth;
        (state, vec![E::SpeakSentence(next)])
    }
}

/// One transition. Total and pure.
pub fn speech_step(mut state: SpeechState, event: SpeechEvent) -> (SpeechState, Vec<SpeechEffect>) {
    match (state.phase, event) {
        (_, V::Mic(mic)) => {
            state.mic = mic;
            match (state.phase, mic, state.pending.clone()) {
                (P::Quiet, MicNow::Closed, Some(wire)) => start(state, &wire),
                _ => (state, vec![]),
            }
        }
        (P::Quiet, V::Speak(wire)) if state.mic == MicNow::Closed => start(state, &wire),
        (P::Quiet | P::Stopping, V::Speak(wire)) => {
            state.pending = Some(wire);
            (state, vec![])
        }
        (P::Synth | P::Playing, V::Speak(wire)) => {
            state.queue.extend(sentences(wire.text.0.as_str()));
            (state, vec![])
        }
        (P::Synth | P::Playing, V::Chunk) => {
            state.phase = P::Playing;
            (state, vec![E::Play])
        }
        (P::Synth | P::Playing, V::SentenceDone) => next_sentence(state),
        (P::Playing, V::Drained) if state.queue.is_empty() => {
            state.phase = P::Quiet;
            state.class = None;
            (state, vec![E::Finished(SpeechEnd::Done)])
        }
        (P::Stopping, V::Drained) => {
            state.phase = P::Quiet;
            state.class = None;
            match (state.mic, state.pending.clone()) {
                (MicNow::Closed, Some(wire)) => start(state, &wire),
                _ => (state, vec![]),
            }
        }
        (P::Synth | P::Playing, V::Hush) => stop(state, SpeechEnd::Hushed),
        (P::Synth | P::Playing, V::Begin) => stop(state, SpeechEnd::BargedIn),
        (_, _) => (state, vec![]),
    }
}
