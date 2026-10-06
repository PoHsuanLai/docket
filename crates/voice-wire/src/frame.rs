//! The framing both ends share: a body in an [`Envelope`] as the text of an `s` argument, and the
//! event fd's frames (a 4-byte big-endian length, then one envelope). A client such as sill reads
//! the fd with [`unframe`] and never links the daemon.
//!
//! A reader loops on [`unframe`]: `Frame` and `Malformed` both say how many bytes to drop (a bad
//! envelope costs one frame, never the stream), `Partial` says read more, and `TooLong` says the
//! stream cannot be trusted any more, so close it.

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

/// The longest frame body either end accepts or writes: 1 MiB. The largest real event is a
/// transcript of a few kilobytes, so this is generous, and it bounds what a reader buffers
/// before it can tell a corrupt length from a slow writer.
pub const MAX_FRAME: u32 = 1 << 20;

/// A frame that cannot be written.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// The body would not serialise.
    #[error("body does not serialise: {0}")]
    Json(#[from] serde_json::Error),
    /// The sealed body is longer than [`MAX_FRAME`].
    #[error("frame of {len} bytes is over the {MAX_FRAME} byte cap")]
    TooLong {
        /// The sealed body's length.
        len: usize,
    },
}

/// One frame of the event fd, refused when its body is over [`MAX_FRAME`].
pub fn frame<T: Serialize>(body: &T) -> Result<Vec<u8>, FrameError> {
    let json = seal(body)?;
    let len = u32::try_from(json.len())
        .ok()
        .filter(|len| *len <= MAX_FRAME)
        .ok_or(FrameError::TooLong { len: json.len() })?;
    Ok(len
        .to_be_bytes()
        .into_iter()
        .chain(json.into_bytes())
        .collect())
}

/// What the front of a read buffer holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unframed<T> {
    /// Not a whole frame yet: read more bytes.
    Partial,
    /// A whole frame: the body, and the bytes to drop.
    Frame {
        /// The decoded body.
        body: T,
        /// The bytes the frame took, header included.
        used: usize,
    },
    /// A whole frame whose envelope does not decode (bad JSON, another vocabulary, an unknown
    /// variant): drop `used` bytes and carry on.
    Malformed {
        /// The bytes the frame took, header included.
        used: usize,
    },
    /// The declared length is over [`MAX_FRAME`]: the stream cannot be resynchronised, close it.
    /// Reported as soon as the four length bytes are in, without waiting for the body.
    TooLong {
        /// The declared length.
        len: u32,
    },
}

/// Reads the first frame of `buffer`.
pub fn unframe<T: DeserializeOwned>(buffer: &[u8]) -> Unframed<T> {
    let Some((head, rest)) = buffer.split_first_chunk::<4>() else {
        return Unframed::Partial;
    };
    let declared = u32::from_be_bytes(*head);
    if declared > MAX_FRAME {
        return Unframed::TooLong { len: declared };
    }
    let len = declared as usize;
    let Some(json) = rest.get(..len) else {
        return Unframed::Partial;
    };
    let used = 4 + len;
    match serde_json::from_slice::<Envelope<T>>(json) {
        Ok(envelope) if envelope.vocab == VoiceVocab::CURRENT => Unframed::Frame {
            body: envelope.body,
            used,
        },
        _ => Unframed::Malformed { used },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Level, VoiceEvent};

    fn level(n: u16) -> Vec<u8> {
        frame(&VoiceEvent::Level(Level(n))).expect("frame")
    }

    fn raw(json: &str) -> Vec<u8> {
        let mut bytes = u32::try_from(json.len())
            .expect("small")
            .to_be_bytes()
            .to_vec();
        bytes.extend_from_slice(json.as_bytes());
        bytes
    }

    #[test]
    fn every_proper_prefix_is_partial_and_the_whole_reads_back() {
        let bytes = level(42);
        for n in 0..bytes.len() {
            assert_eq!(unframe::<VoiceEvent>(&bytes[..n]), Unframed::Partial, "{n}");
        }
        assert_eq!(
            unframe::<VoiceEvent>(&bytes),
            Unframed::Frame {
                body: VoiceEvent::Level(Level(42)),
                used: bytes.len()
            }
        );
    }

    #[test]
    fn a_malformed_frame_is_skipped_and_the_next_one_reads() {
        for bad in [
            raw("not json"),
            raw(r#"{"vocab":2,"body":{"kind":"level","v":1}}"#),
            raw(&format!(
                r#"{{"vocab":{},"body":"no_such_event"}}"#,
                VoiceVocab::CURRENT.0
            )),
        ] {
            let good = level(7);
            let buffer: Vec<u8> = bad.iter().chain(&good).copied().collect();
            let Unframed::Malformed { used } = unframe::<VoiceEvent>(&buffer) else {
                panic!("not malformed: {bad:?}");
            };
            assert_eq!(used, bad.len());
            assert_eq!(
                unframe::<VoiceEvent>(&buffer[used..]),
                Unframed::Frame {
                    body: VoiceEvent::Level(Level(7)),
                    used: good.len()
                }
            );
        }
    }

    #[test]
    fn an_over_cap_length_is_too_long_without_its_bytes() {
        let len = MAX_FRAME + 1;
        assert_eq!(
            unframe::<VoiceEvent>(&len.to_be_bytes()),
            Unframed::TooLong { len }
        );
        let at_cap = MAX_FRAME.to_be_bytes();
        assert_eq!(unframe::<VoiceEvent>(&at_cap), Unframed::Partial);
    }

    #[test]
    fn frame_refuses_an_over_cap_body() {
        let body = "x".repeat(MAX_FRAME as usize);
        assert!(matches!(
            frame(&body),
            Err(FrameError::TooLong { len }) if len > MAX_FRAME as usize
        ));
        assert!(frame(&"x").is_ok());
    }
}
