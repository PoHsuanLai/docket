//! The framing both ends share: a body in an [`Envelope`] as the text of an `s` argument, and the
//! event fd's frames (a 4-byte big-endian length, then one envelope). A client such as sill reads
//! the fd with [`unframe`] and never links the daemon.

use crate::text::{Envelope, VoiceVocab};
use serde::Serialize;
use serde::de::DeserializeOwned;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Level, VoiceEvent};

    #[test]
    fn a_frame_reads_back_and_a_short_buffer_is_partial() {
        let bytes = frame(&VoiceEvent::Level(Level(42))).expect("frame");
        let (event, used) = unframe::<VoiceEvent>(&bytes).expect("whole");
        assert_eq!((event, used), (VoiceEvent::Level(Level(42)), bytes.len()));
        assert!(unframe::<VoiceEvent>(&bytes[..bytes.len() - 1]).is_none());
        assert!(unframe::<VoiceEvent>(&bytes[..2]).is_none());
    }
}
