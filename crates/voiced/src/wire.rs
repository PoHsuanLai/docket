//! Bodies on the bus and frames on the fd: `voice-wire` types in an `Envelope`, JSON in `s`; the
//! event fd carries a 4-byte big-endian length then one envelope (porter-core's framing, with
//! `VoiceVocab`). Audio never appears here. The framing itself is `voice-wire`'s, so a client
//! such as sill needs no daemon crate.

use crate::error::VoiceError;
use serde::de::DeserializeOwned;
use voice_wire::{CancelCause, Envelope, VoiceVocab};
pub use voice_wire::{FrameError, MAX_FRAME, Unframed, frame, seal, unframe};
use zbus::zvariant::OwnedObjectPath;

/// The body of an envelope the caller sent, refused when it is another vocabulary.
pub(crate) fn open<T: DeserializeOwned>(text: &str) -> Result<T, VoiceError> {
    let envelope: Envelope<T> =
        serde_json::from_str(text).map_err(|e| VoiceError::Malformed(e.to_string()))?;
    if envelope.vocab == VoiceVocab::CURRENT {
        Ok(envelope.body)
    } else {
        Err(VoiceError::Malformed(format!(
            "vocabulary {} (this daemon speaks {})",
            envelope.vocab.0,
            VoiceVocab::CURRENT.0
        )))
    }
}

/// A cancel cause a caller may give: the daemon's own (`Superseded`, `TooLong`) are refused as a
/// malformed body, like any other body that is not this method's.
pub(crate) fn caller_cause(text: &str) -> Result<CancelCause, VoiceError> {
    match open::<CancelCause>(text)? {
        cause @ (CancelCause::Escape
        | CancelCause::OtherInput
        | CancelCause::FocusLost
        | CancelCause::Shell) => Ok(cause),
        CancelCause::Superseded | CancelCause::TooLong => Err(VoiceError::Malformed(
            "that cancel cause is the daemon's own".to_owned(),
        )),
    }
}

pub(crate) fn path(text: &str) -> Result<OwnedObjectPath, VoiceError> {
    OwnedObjectPath::try_from(text).map_err(|e| VoiceError::Malformed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_callers_causes_are_accepted() {
        for (cause, allowed) in [
            (CancelCause::Escape, true),
            (CancelCause::OtherInput, true),
            (CancelCause::FocusLost, true),
            (CancelCause::Shell, true),
            (CancelCause::Superseded, false),
            (CancelCause::TooLong, false),
        ] {
            let got = caller_cause(&seal(&cause).expect("seal"));
            assert_eq!(got.is_ok(), allowed, "{cause:?}");
        }
        assert!(caller_cause("").is_err());
    }

    #[test]
    fn a_foreign_vocabulary_is_refused() {
        let text = r#"{"vocab":2,"body":"opened"}"#;
        assert!(matches!(
            open::<String>(text),
            Err(VoiceError::Malformed(_))
        ));
        let ours = seal(&"x").expect("seal");
        assert_eq!(open::<String>(&ours).expect("ours"), "x");
    }
}
