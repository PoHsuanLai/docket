//! The voice event fd: arbitrary bytes, read the way a client reads them.
#![no_main]

use libfuzzer_sys::fuzz_target;
use voice_wire::{MAX_FRAME, Unframed, VoiceEvent, unframe};

fuzz_target!(|bytes: &[u8]| {
    let mut rest = bytes;
    loop {
        match unframe::<VoiceEvent>(rest) {
            Unframed::Frame { used, .. } | Unframed::Malformed { used } => {
                assert!(used >= 4 && used <= rest.len());
                rest = &rest[used..];
            }
            Unframed::Partial => break,
            Unframed::TooLong { len } => {
                assert!(len > MAX_FRAME);
                break;
            }
        }
    }
});
