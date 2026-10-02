//! What flows on the utterance fd.

use crate::text::{HeardSegment, HeardTail, HeardText};
use porter_infer::{InferRefusal, ModelError, Readiness, ServedBy};
use serde::{Deserialize, Serialize};

/// An input level in thousandths, 0 to 1000.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Level(pub u16);

/// Why an utterance was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelCause {
    /// Escape.
    Escape,
    /// Another key or the pointer.
    OtherInput,
    /// Focus left the field.
    FocusLost,
    /// A newer utterance began.
    Superseded,
    /// Held too long.
    TooLong,
    /// The shell cancelled.
    Shell,
}

/// Why an utterance failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum VoiceFault {
    /// No microphone.
    MicUnavailable,
    /// The microphone was denied.
    MicDenied,
    /// No speech model.
    NoModel,
    /// The engine failed.
    Engine(ModelError),
    /// inferd refused.
    Refused(InferRefusal),
}

/// How an utterance ended. The text goes on the fds only; the unicast `Ended` signal carries
/// the same value with `Heard` text removed by the caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum UtteranceEnd {
    /// Heard this.
    Heard {
        /// Everything said.
        text: HeardText,
        /// Who transcribed it.
        served: ServedBy,
    },
    /// Nothing recognisable.
    NothingHeard,
    /// Cancelled.
    Cancelled(CancelCause),
    /// Failed.
    Failed(VoiceFault),
}

/// One frame on an attached client's fd.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum VoiceEvent {
    /// The mic stream is running.
    Opened,
    /// The engine is cold: audio is buffered, never dropped.
    Waiting(Readiness),
    /// The input level, at most 31 per second, while listening only.
    Level(Level),
    /// The unstable tail.
    Partial(HeardTail),
    /// A stable segment.
    Committed(HeardSegment),
    /// Over.
    Ended(UtteranceEnd),
}
