//! The voice path into a field: `docket-ds` attaches to the utterance the shell opened
//! (`Voice1.Utterance.Attach`), reads the `VoiceEvent` frames and hands the words to ds's
//! prompt or dictation machine. The mapping from a frame to what a field does with it is pure.

use voice_wire::{UtteranceEnd, VoiceEvent};

/// What a field does with a voice frame.
#[derive(Clone, PartialEq, Eq)]
pub enum Heard {
    /// The input level, 0 to 1000 (drives the orb's level only while listening).
    Level(u16),
    /// The unstable tail: shown provisional, not in the model.
    Tail(String),
    /// A stable segment: dictation inserts it.
    Committed(String),
    /// The utterance ended.
    Ended(HeardEnd),
}

// The words are the person's: Debug shows shapes only.
impl std::fmt::Debug for Heard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Heard::Level(n) => write!(f, "Level({n})"),
            Heard::Tail(t) => write!(f, "Tail(<{} bytes>)", t.len()),
            Heard::Committed(t) => write!(f, "Committed(<{} bytes>)", t.len()),
            Heard::Ended(e) => write!(f, "Ended({e:?})"),
        }
    }
}

/// How an utterance ended, for a field.
#[derive(Clone, PartialEq, Eq)]
pub enum HeardEnd {
    /// Send these words.
    Send(String),
    /// Nothing was heard (the field says "Didn't catch that").
    Nothing,
    /// Cancelled or failed: the field restores what it had.
    Cancelled,
}

impl std::fmt::Debug for HeardEnd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HeardEnd::Send(t) => write!(f, "Send(<{} bytes>)", t.len()),
            HeardEnd::Nothing => f.write_str("Nothing"),
            HeardEnd::Cancelled => f.write_str("Cancelled"),
        }
    }
}

/// The field's view of one frame; frames a field has no use for (`Opened`, `Waiting`) give none.
pub fn heard_of(event: &VoiceEvent) -> Option<Heard> {
    match event {
        VoiceEvent::Opened | VoiceEvent::Waiting(_) => None,
        VoiceEvent::Level(level) => Some(Heard::Level(level.0)),
        VoiceEvent::Partial(tail) => Some(Heard::Tail(tail.text.0.clone())),
        VoiceEvent::Committed(segment) => Some(Heard::Committed(segment.text.0.clone())),
        VoiceEvent::Ended(UtteranceEnd::Heard { text, .. }) => {
            Some(Heard::Ended(HeardEnd::Send(text.0.clone())))
        }
        VoiceEvent::Ended(UtteranceEnd::NothingHeard) => Some(Heard::Ended(HeardEnd::Nothing)),
        VoiceEvent::Ended(UtteranceEnd::Cancelled(_) | UtteranceEnd::Failed(_)) => {
            Some(Heard::Ended(HeardEnd::Cancelled))
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
