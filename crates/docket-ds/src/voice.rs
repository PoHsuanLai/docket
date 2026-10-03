//! The voice path into a field: `docket-ds` attaches to the utterance the shell opened
//! (`Voice1.Utterance.Attach`), reads the `VoiceEvent` frames and hands the words to ds's
//! prompt or dictation machine. The mapping from a frame to what a field does with it is pure.

use ds_intents::{HeardEndMark, HeardMark, InputLevel};
use voice_wire::{UtteranceEnd, VoiceEvent};

/// The field's view of one frame; frames a field has no use for (`Opened`, `Waiting`) give none.
pub fn heard_of(event: &VoiceEvent) -> Option<HeardMark> {
    match event {
        VoiceEvent::Opened | VoiceEvent::Waiting(_) => None,
        VoiceEvent::Level(level) => Some(HeardMark::Level(InputLevel(level.0))),
        VoiceEvent::Partial(tail) => Some(HeardMark::Tail(tail.text.0.clone())),
        VoiceEvent::Committed(segment) => Some(HeardMark::Committed(segment.text.0.clone())),
        VoiceEvent::Ended(UtteranceEnd::Heard { text, .. }) => {
            Some(HeardMark::Ended(HeardEndMark::Send(text.0.clone())))
        }
        VoiceEvent::Ended(UtteranceEnd::NothingHeard) => {
            Some(HeardMark::Ended(HeardEndMark::Nothing))
        }
        VoiceEvent::Ended(UtteranceEnd::Cancelled(_) | UtteranceEnd::Failed(_)) => {
            Some(HeardMark::Ended(HeardEndMark::Cancelled))
        }
    }
}

/// Carries an attached utterance to a field: an app implements it over ds's prompt and
/// dictation ports.
pub trait DictationBridge: Send + Sync {
    /// Attaches to the utterance the shell routed here and feeds each frame to the field until
    /// it ends. Only the routed app may attach; a secure field answers `Declined` before this
    /// is ever called.
    fn attach(&self) -> impl std::future::Future<Output = Result<(), crate::BridgeFault>> + Send;
}
