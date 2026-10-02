//! Summoning the companion into an app: what `IntentProvider1.Summon(serial, origin)` carries
//! and how the app answers.

use crate::ids::UtteranceId;
use serde::{Deserialize, Serialize};

/// One summon, as sill numbers it so a late answer cannot be taken for a newer one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SummonSerial(pub u64);

/// What a voice utterance is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceIntent {
    /// A prompt for the companion.
    Ask,
    /// Text for the focused field.
    Dictate,
}

/// How the companion was summoned.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum SummonOrigin {
    /// The double-tap of ⌘.
    DoubleTap,
    /// A held gesture opened the microphone; the app attaches to this utterance.
    Voice {
        /// The utterance.
        utterance: UtteranceId,
        /// What it is for.
        intent: VoiceIntent,
    },
}

/// How an app took a summon. Dictation answers `TookField` (a focused editable, non-secure
/// field) or `Declined`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummonAnswer {
    /// The focused field became the prompt.
    TookField,
    /// An anchored prompt appeared at the selection.
    TookAnchored,
    /// The previous prompt was restored.
    Restored,
    /// Not here.
    Declined,
}
