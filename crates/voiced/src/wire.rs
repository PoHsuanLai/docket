//! Bodies on the bus and frames on the fd: `voice-wire` types in an `Envelope`, JSON in `s`; the
//! event fd carries a 4-byte big-endian length then one envelope (porter-core's framing, with
//! `VoiceVocab`). Audio never appears here. The framing itself is `voice-wire`'s, so a client
//! such as sill needs no daemon crate.

use crate::error::VoiceError;
use serde::de::DeserializeOwned;
use voice_wire::{Envelope, VoiceVocab};
pub use voice_wire::{frame, seal, unframe};
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

pub(crate) fn path(text: &str) -> Result<OwnedObjectPath, VoiceError> {
    OwnedObjectPath::try_from(text).map_err(|e| VoiceError::Malformed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
