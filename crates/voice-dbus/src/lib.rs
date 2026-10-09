//! The caller's side of `org.quire.Voice1`: proxies and the bus names. A shell or an app that
//! only calls the voice service links this crate, never the microphone daemon (`voiced`, which
//! re-exports these names). Bodies are `voice-wire` types.

mod names;
mod proxy;

pub use names::{
    SPEECH_PREFIX, UTTERANCE_PREFIX, VOICE_BUS, VOICE_PATH, speech_path, utterance_path,
};
pub use proxy::{SpeechProxy, UtteranceProxy, VoiceProxy};
