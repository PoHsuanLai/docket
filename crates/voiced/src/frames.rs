//! The pure part of hearing, shared by the daemon's loop (`serve`) and in-process dictation
//! (`dictate`): capture chunks cut into whole 32 ms frames, and the silence after speech that ends
//! a dictation. Neither touches the bus, a device or the engine, so both callers step the same
//! way and a test can drive them with plain samples.

use speech_provider::{Frame512, SampleIndex, VoiceActivity, Voiced};
use speech_vad::{Endpoint, EndpointParams, EnergyGate, EnergyGateParams, endpoint};

/// Samples in one frame (32 ms at 16 kHz).
pub(crate) const FRAME: usize = 512;

/// Capture chunks of any size, cut into whole frames in order. What is left over waits for the
/// next chunk; the end of the stream drops it.
#[derive(Debug, Default)]
pub(crate) struct Framer {
    carry: Vec<i16>,
}

impl Framer {
    /// Adds a chunk of capture.
    pub(crate) fn push(&mut self, samples: Vec<i16>) {
        self.carry.extend(samples);
    }

    /// The next whole frame, if there is one.
    pub(crate) fn next_frame(&mut self) -> Option<(Vec<i16>, Frame512)> {
        if self.carry.len() < FRAME {
            return None;
        }
        let pcm: Vec<i16> = self.carry.drain(..FRAME).collect();
        let framed = Frame512::new(&pcm).ok()?;
        Some((pcm, framed))
    }

    /// Forgets what is left over (the utterance's audio is wiped).
    pub(crate) fn clear(&mut self) {
        self.carry.clear();
    }
}

/// The silence detector of a dictation: it ends the utterance after the silence it is told.
#[derive(Debug)]
pub(crate) struct Dictation {
    gate: EnergyGate,
    state: Endpoint,
    at: u64,
}

impl Dictation {
    pub(crate) fn new() -> Self {
        Self {
            gate: EnergyGate::new(EnergyGateParams::default()),
            state: Endpoint::Waiting,
            at: 0,
        }
    }

    /// Whether this frame ended it (after speech or never any).
    pub(crate) fn ended(&mut self, frame: &Frame512) -> bool {
        let (voiced, _): (Voiced, _) = self.gate.push(frame);
        self.state = endpoint(
            self.state,
            voiced,
            SampleIndex(self.at),
            &EndpointParams::default(),
        );
        self.at += FRAME as u64;
        matches!(self.state, Endpoint::Ended(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_are_cut_into_whole_frames_and_the_rest_waits() {
        let mut framer = Framer::default();
        framer.push(vec![1; 700]);
        let first = framer.next_frame().expect("a frame");
        assert_eq!(first.0.len(), FRAME);
        assert!(framer.next_frame().is_none(), "188 samples wait");
        framer.push(vec![2; 400]);
        let (second, _) = framer.next_frame().expect("another");
        assert_eq!(&second[..188], &[1; 188][..]);
        assert_eq!(second[188], 2);
        assert!(framer.next_frame().is_none());
    }

    #[test]
    fn clearing_forgets_the_leftover() {
        let mut framer = Framer::default();
        framer.push(vec![0; 600]);
        framer.clear();
        framer.push(vec![0; 400]);
        assert!(framer.next_frame().is_none());
    }

    #[test]
    fn a_dictation_that_hears_nothing_has_not_ended_after_one_frame() {
        let mut dictation = Dictation::new();
        let frame = Frame512::new(&[0; FRAME]).expect("frame");
        assert!(!dictation.ended(&frame));
    }
}
