//! The wire of `org.quire.Voice1` (voice.md §3.5): serde-only bodies, no zbus. Bodies travel as
//! JSON in `s` inside an [`Envelope`]; utterance events travel as frames on a passed fd. Audio
//! never appears here, and the transcript fields redact their `Debug`.

mod begin;
mod event;
mod frame;
mod refusal;
mod status;
mod text;

pub use begin::{VoiceBegin, VoiceTarget, VoiceTrigger};
pub use event::{CancelCause, Level, UtteranceEnd, VoiceEvent, VoiceFault};
pub use frame::{frame, seal, unframe};
pub use refusal::{ERROR_PREFIX, VoiceRefusal};
pub use status::{MicState, Speaking, SpeechEnd, VoiceStatus, VoiceUse};
pub use text::{Envelope, HeardSegment, HeardTail, HeardText, SpeakWire, SpokenText, VoiceVocab};
