//! voiced: the microphone's one owner. It opens capture only on `Voice1.Begin` from the shell
//! role, runs the pure machines of `voice-loop`, talks to inferd through inferd-client, and
//! records nothing. The audio device (`voice-audio`), inferd, the engine warmer and the
//! person's consent are seams (`Seams`), so tests serve the whole daemon on a private bus with no real device.

mod bus;
mod callers;
mod command;
mod config;
mod dictate;
mod engine;
mod error;
mod frames;
mod hear;
mod introspect;
mod link;
mod peer;
mod playback;
mod serve;
mod sink;
mod talk;
mod usage;
mod utter;
mod warm;
mod watch;
mod wire;

pub use bus::{SpeechSkeleton, UtteranceSkeleton, VoiceSkeleton};
pub use callers::{FILE as CALLERS_FILE, read as read_callers, shipped as shipped_callers};
pub use config::{ConfigError, Earcons, VoiceRole, VoicedConfig};
pub use dictate::{Dictated, dictate};
pub use error::{VoiceError, refusal_of_name};
pub use introspect::{VOICE1_FILE, introspection};
pub use peer::Peers;
pub use playback::{EARCON_RATE, earcon_samples};
pub use serve::{Running, Seams, ServeError, serve, start, supervise};
pub use usage::{FileUse, FixedUse, UseSource, use_of_settings};
pub use voice_dbus::{
    SPEECH_PREFIX, SpeechProxy, UTTERANCE_PREFIX, UtteranceProxy, VOICE_BUS, VOICE_PATH,
    VoiceProxy, speech_path, utterance_path,
};
pub use warm::{BusWarm, FixedWarm, TransportWarm, Warm};
pub use wire::{FrameError, MAX_FRAME, Unframed, frame, seal, unframe};
