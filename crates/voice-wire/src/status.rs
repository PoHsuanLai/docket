//! Content-free status of the voice service.

use crate::event::VoiceFault;
use docket_core::VoiceIntent;
use porter_core::UnixSeconds;
use porter_infer::Readiness;
use serde::{Deserialize, Serialize};

/// The microphone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum MicState {
    /// Closed.
    Closed,
    /// Open since then, for this intent.
    Open {
        /// When it opened.
        since: UnixSeconds,
        /// What for.
        intent: VoiceIntent,
    },
}

/// Whether speech output is playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Speaking {
    /// Silent.
    Quiet,
    /// Playing.
    Speaking,
}

/// Whether voice may be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceUse {
    /// Off in settings.
    Off,
    /// Waiting for the first-use consent.
    NeedsConsent,
    /// On.
    On,
}

/// What `Voice1.Status` answers: no content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceStatus {
    /// The mic.
    pub mic: MicState,
    /// Whether it is speaking.
    pub speaking: Speaking,
    /// The speech-to-text engine.
    pub stt: Readiness,
    /// The text-to-speech engine.
    pub tts: Readiness,
    /// Whether voice is enabled.
    pub enabled: VoiceUse,
}

/// How speech ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum SpeechEnd {
    /// Finished.
    Done,
    /// Hushed.
    Hushed,
    /// An utterance began.
    BargedIn,
    /// Failed.
    Failed(VoiceFault),
}
