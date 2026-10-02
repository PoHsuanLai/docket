//! The voice machines, pure (voice.md §4.2, §4.3). Time and audio are inputs; devices and
//! inference are effects the daemon carries out. Every step input and output is `Serialize + Eq`
//! so a trace can replay it.
//!
//! The invariant pinned here: the microphone is open exactly in the states `Opening`,
//! `Listening` and `Tail`. No other state, input or setting opens it.

mod buffer;
mod coordinate;
mod sentencer;
mod speech;
mod utterance;

pub use buffer::{BUFFER_SAMPLES, PcmBuffer, PushOutcome};
pub use coordinate::{Step, begin_utterance};
pub use sentencer::{MAX_SENTENCE_CHARS, MIN_SENTENCE_CHARS, Sentence, sentences};
pub use speech::{FADE_MS, SpeechEffect, SpeechEvent, SpeechPhase, SpeechState, speech_step};
pub use utterance::{
    Earcon, EngineGate, MicNow, TAIL_MS, Transcript, UtteranceEffect, UtteranceEvent,
    UtteranceState, mic_of, utterance_step,
};
