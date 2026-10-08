//! voiced: the microphone's one owner. It opens capture only on `Voice1.Begin` from the shell
//! role, runs the pure machines of `voice-loop`, talks to inferd through porter-client, and
//! records nothing. The audio device, inferd, the engine warmer and the person's consent are
//! seams (`Seams`), so tests serve the whole daemon on a private bus with no real device.

mod bus;
mod command;
mod config;
mod default_source;
mod device;
mod engine;
mod error;
mod hear;
mod introspect;
mod link;
mod names;
mod peer;
mod pipewire_device;
mod playback;
mod serve;
mod sink;
mod talk;
mod usage;
mod utter;
mod warm;
mod wire;

pub use bus::{
    SpeechProxy, SpeechSkeleton, UtteranceProxy, UtteranceSkeleton, VoiceProxy, VoiceSkeleton,
};
pub use config::{ConfigError, Earcons, VoiceRole, VoicedConfig};
pub use default_source::{CONFIGURED_KEY, DefaultSources, RESOLVED_KEY, name_in};
pub use device::{
    AudioDevice, AudioNode, CaptureFormat, CaptureStream, Chosen, ChosenBy, DeviceError,
    MediaClass, NodeId, NodeKind, PlaybackFormat, PlaybackStream, SNAPSHOT_BUDGET, Snapshot,
    choose_capture, snapshot_before,
};
#[cfg(feature = "testing")]
pub use device::{FakeAudioDevice, FakeCapture, FakePlayback};
pub use error::{VoiceError, refusal_of_name};
pub use introspect::{VOICE1_FILE, introspection};
pub use names::{
    SPEECH_PREFIX, UTTERANCE_PREFIX, VOICE_BUS, VOICE_PATH, speech_path, utterance_path,
};
pub use peer::Peers;
pub use pipewire_device::PipeWireDevice;
pub use playback::{EARCON_RATE, earcon_samples};
pub use serve::{Running, Seams, serve, start};
pub use usage::{FileUse, FixedUse, UseSource, use_of_settings};
pub use warm::{BusWarm, FixedWarm, TransportWarm, Warm};
pub use wire::{FrameError, MAX_FRAME, Unframed, frame, seal, unframe};
