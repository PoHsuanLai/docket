//! Refusals and their D-Bus error names, 1:1.

use serde::{Deserialize, Serialize};

/// The prefix of every error name of this interface.
pub const ERROR_PREFIX: &str = "org.quire.Voice1.Error.";

/// Why a call was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum VoiceRefusal {
    /// Voice is off.
    #[error("voice is disabled")]
    Disabled,
    /// First-use consent is pending.
    #[error("voice needs consent")]
    NeedsConsent,
    /// Busy.
    #[error("busy")]
    Busy,
    /// This caller may not.
    #[error("not allowed")]
    NotAllowed,
    /// No speech model.
    #[error("no model")]
    NoModel,
    /// No microphone.
    #[error("microphone unavailable")]
    MicUnavailable,
}

impl VoiceRefusal {
    /// Every refusal.
    pub const ALL: [VoiceRefusal; 6] = [
        VoiceRefusal::Disabled,
        VoiceRefusal::NeedsConsent,
        VoiceRefusal::Busy,
        VoiceRefusal::NotAllowed,
        VoiceRefusal::NoModel,
        VoiceRefusal::MicUnavailable,
    ];

    /// The full D-Bus error name.
    pub fn error_name(self) -> String {
        let variant = match self {
            VoiceRefusal::Disabled => "Disabled",
            VoiceRefusal::NeedsConsent => "NeedsConsent",
            VoiceRefusal::Busy => "Busy",
            VoiceRefusal::NotAllowed => "NotAllowed",
            VoiceRefusal::NoModel => "NoModel",
            VoiceRefusal::MicUnavailable => "MicUnavailable",
        };
        format!("{ERROR_PREFIX}{variant}")
    }

    /// The refusal a D-Bus error name stands for.
    pub fn from_error_name(name: &str) -> Option<VoiceRefusal> {
        Self::ALL.into_iter().find(|r| r.error_name() == name)
    }
}
