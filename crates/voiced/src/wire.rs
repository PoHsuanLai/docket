//! Bodies on the bus and frames on the fd: `voice-wire` types in an `Envelope`, JSON in `s`; the
//! event fd carries a 4-byte big-endian length then one envelope (porter-core's framing, with
//! `VoiceVocab`). Audio never appears here.

use crate::error::VoiceError;
use serde::Serialize;
use serde::de::DeserializeOwned;
use voice_wire::{Envelope, VoiceVocab};
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

/// `body` in an envelope, as the text of an `s` argument.
pub fn seal<T: Serialize>(body: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string(&Envelope {
        vocab: VoiceVocab::CURRENT,
        body,
    })
}

/// One frame of the event fd.
pub fn frame<T: Serialize>(body: &T) -> Result<Vec<u8>, serde_json::Error> {
    let json = seal(body)?;
    let len = u32::try_from(json.len()).unwrap_or(u32::MAX);
    Ok(len
        .to_be_bytes()
        .into_iter()
        .chain(json.into_bytes())
        .collect())
}

/// Reads the first frame of `buffer`: the body and the bytes it used, or `None` while partial.
pub fn unframe<T: DeserializeOwned>(buffer: &[u8]) -> Option<(T, usize)> {
    let (head, rest) = buffer.split_first_chunk::<4>()?;
    let len = u32::from_be_bytes(*head) as usize;
    let json = rest.get(..len)?;
    let envelope: Envelope<T> = serde_json::from_slice(json).ok()?;
    Some((envelope.body, 4 + len))
}

pub(crate) fn path(text: &str) -> Result<OwnedObjectPath, VoiceError> {
    OwnedObjectPath::try_from(text).map_err(|e| VoiceError::Malformed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use voice_wire::{Level, VoiceEvent};

    #[test]
    fn a_frame_reads_back_and_a_short_buffer_is_partial() {
        let bytes = frame(&VoiceEvent::Level(Level(42))).expect("frame");
        let (event, used) = unframe::<VoiceEvent>(&bytes).expect("whole");
        assert_eq!((event, used), (VoiceEvent::Level(Level(42)), bytes.len()));
        assert!(unframe::<VoiceEvent>(&bytes[..bytes.len() - 1]).is_none());
        assert!(unframe::<VoiceEvent>(&bytes[..2]).is_none());
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
