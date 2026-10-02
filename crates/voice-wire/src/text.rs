//! Envelope and the transcript text types.

use docket_core::UtteranceId;
use porter_core::DataClass;
use porter_core::capability::LanguageTag;
use serde::{Deserialize, Serialize};
use std::fmt;

/// The version of this wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VoiceVocab(pub u32);

impl VoiceVocab {
    /// What this build speaks.
    pub const CURRENT: VoiceVocab = VoiceVocab(1);
}

/// A body with its vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope<T> {
    /// The vocabulary.
    pub vocab: VoiceVocab,
    /// The body.
    pub body: T,
}

macro_rules! spoken {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        // What the person said: Debug shows the length only.
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "(<{} bytes>)"), self.0.len())
            }
        }
    };
}

spoken!(
    /// Recognised words.
    HeardText
);
spoken!(
    /// Text to speak.
    SpokenText
);

/// The unstable tail after what is committed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeardTail {
    /// The words.
    pub text: HeardText,
}

/// A stable segment; never revised. Dictation inserts it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeardSegment {
    /// The words.
    pub text: HeardText,
}

/// A request to speak. The class is the text's own, so a mail summary is never spoken by a
/// cloud voice unless mail's floor allows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeakWire {
    /// The utterance this answers, if any.
    pub utterance: Option<UtteranceId>,
    /// What to say.
    pub text: SpokenText,
    /// The data class of the text.
    pub class: DataClass,
    /// The language.
    pub lang: LanguageTag,
}
