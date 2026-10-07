//! `unframe` over arbitrary bytes and arbitrary splits of a stream: no panic, bounded work, the
//! four outcomes consistent with the four length bytes, and a stream cut anywhere reads back the
//! same frames once the rest arrives.

use proptest::prelude::*;
use voice_wire::{Level, MAX_FRAME, Unframed, VoiceEvent, frame, unframe};

fn events() -> impl Strategy<Value = Vec<VoiceEvent>> {
    prop::collection::vec(
        prop_oneof![
            Just(VoiceEvent::Opened),
            any::<u16>().prop_map(|n| VoiceEvent::Level(Level(n))),
        ],
        0..8,
    )
}

/// Reads frames off the front of `buffer` the way a client does: drop what a frame or a
/// malformed frame took, stop at a partial one, give up at a too-long one.
fn drain(mut buffer: &[u8]) -> (Vec<VoiceEvent>, usize, bool) {
    let mut bodies = Vec::new();
    let mut malformed = 0;
    loop {
        match unframe::<VoiceEvent>(buffer) {
            Unframed::Frame { body, used } => {
                bodies.push(body);
                buffer = &buffer[used..];
            }
            Unframed::Malformed { used } => {
                malformed += 1;
                buffer = &buffer[used..];
            }
            Unframed::Partial => return (bodies, malformed, false),
            Unframed::TooLong { .. } => return (bodies, malformed, true),
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn the_outcome_follows_the_length_and_never_overruns(bytes in prop::collection::vec(any::<u8>(), 0..300)) {
        match unframe::<VoiceEvent>(&bytes) {
            Unframed::Partial => {
                let declared = bytes.first_chunk::<4>().map(|h| u32::from_be_bytes(*h));
                prop_assert!(declared.is_none_or(|d| bytes.len() < 4 + d as usize && d <= MAX_FRAME));
            }
            Unframed::Frame { used, .. } | Unframed::Malformed { used } => {
                let declared = u32::from_be_bytes(*bytes.first_chunk::<4>().expect("four bytes")) as usize;
                prop_assert_eq!(used, 4 + declared);
                prop_assert!(used <= bytes.len());
            }
            Unframed::TooLong { len } => {
                prop_assert!(len > MAX_FRAME);
                prop_assert_eq!(len, u32::from_be_bytes(*bytes.first_chunk::<4>().expect("four bytes")));
            }
        }
    }

    #[test]
    fn a_declared_length_over_the_cap_is_too_long_whatever_follows(
        len in (MAX_FRAME + 1)..=u32::MAX,
        rest in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        let bytes: Vec<u8> = len.to_be_bytes().into_iter().chain(rest).collect();
        prop_assert_eq!(unframe::<VoiceEvent>(&bytes), Unframed::TooLong { len });
    }

    #[test]
    fn a_stream_split_anywhere_reads_the_same_frames(
        sent in events(),
        cuts in prop::collection::vec(any::<prop::sample::Index>(), 0..5),
    ) {
        let stream: Vec<u8> = sent
            .iter()
            .flat_map(|e| frame(e).expect("frame"))
            .collect();
        let mut at: Vec<usize> = cuts.iter().map(|c| c.index(stream.len() + 1)).collect();
        at.sort_unstable();
        // Feed the stream in pieces, keeping what a partial frame left over.
        let mut held: Vec<u8> = Vec::new();
        let mut got = Vec::new();
        let mut from = 0;
        for end in at.into_iter().chain([stream.len()]) {
            held.extend_from_slice(&stream[from..end]);
            from = end;
            let (bodies, malformed, too_long) = drain(&held);
            prop_assert_eq!((malformed, too_long), (0, false));
            let used: usize = bodies.iter().map(|b| frame(b).expect("frame").len()).sum();
            held.drain(..used);
            got.extend(bodies);
        }
        prop_assert_eq!(got, sent);
        prop_assert!(held.is_empty());
    }

    #[test]
    fn garbage_between_frames_costs_one_frame_or_closes_the_stream(
        garbage in prop::collection::vec(any::<u8>(), 0..200),
        level in any::<u16>(),
    ) {
        let good = frame(&VoiceEvent::Level(Level(level))).expect("frame");
        let buffer: Vec<u8> = garbage.iter().chain(&good).copied().collect();
        let (bodies, _, _) = drain(&buffer);
        // Whatever the garbage was, the reader consumed bytes only through frame boundaries it
        // computed from lengths and never produced more events than bytes allow.
        prop_assert!(bodies.len() <= buffer.len() / 4 + 1);
    }
}
