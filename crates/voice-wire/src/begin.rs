//! Opening an utterance.

use docket_core::VoiceIntent;
use porter_core::AppName;
use porter_core::SpaceId;
use serde::{Deserialize, Serialize};

/// What started the utterance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceTrigger {
    /// Holding the command key.
    HoldKey,
    /// Holding the pointer on the orb.
    HoldPointer,
    /// The dictation key.
    DictationKey,
    /// Edit, Start Dictation.
    DictationMenu,
}

/// `Voice1.Begin`: opens the mic now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceBegin {
    /// Ask or dictate.
    pub intent: VoiceIntent,
    /// What started it.
    pub trigger: VoiceTrigger,
    /// The Space the utterance belongs to.
    pub space: SpaceId,
}

/// Who may attach to the utterance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum VoiceTarget {
    /// The shell shows it.
    Shell,
    /// This app, for this summon.
    App {
        /// The app (checked with `GetNameOwner`).
        app: AppName,
        /// The summon serial.
        serial: u64,
    },
}
