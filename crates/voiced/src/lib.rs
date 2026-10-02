//! voiced: the microphone's one owner. It opens capture only on `Voice1.Begin` from the shell
//! role, runs the pure machines of `voice-loop`, talks to inferd through porter-client, and
//! records nothing. This crate holds the seams, the configuration and the bus skeleton; the
//! PipeWire device and the serving loop are frozen signatures.

mod bus;
mod config;
mod device;
mod introspect;
mod names;
mod serve;

pub use bus::{
    SpeechProxy, SpeechSkeleton, UtteranceProxy, UtteranceSkeleton, VoiceProxy, VoiceSkeleton,
};
pub use config::{ConfigError, Earcons, VoiceRole, VoicedConfig};
pub use device::{
    AudioDevice, AudioNode, CaptureFormat, CaptureStream, DeviceError, MediaClass, NodeId,
    NodeKind, PlaybackFormat, PlaybackStream, choose_capture,
};
#[cfg(feature = "testing")]
pub use device::{FakeAudioDevice, FakeCapture, FakePlayback};
pub use introspect::{VOICE1_FILE, introspection};
pub use names::{
    SPEECH_PREFIX, UTTERANCE_PREFIX, VOICE_BUS, VOICE_PATH, speech_path, utterance_path,
};
pub use serve::serve;
