//! The pure part of hearing, shared by the daemon's loop (`serve`) and in-process dictation
//! (`dictate`): capture chunks cut into whole 32 ms frames, and the silence after speech that ends
//! a dictation. Neither touches the bus, a device or the engine, so both callers step the same
//! way and a test can drive them with plain samples.

use speech_provider::{
    AudioChunk, AudioFormat, FRAME_SAMPLES, Frame512, PcmBytes, PcmFormat, SampleIndex,
    VoiceActivity, Voiced,
};
use speech_vad::{Endpoint, EndpointParams, EnergyGate, EnergyGateParams, Framer, endpoint};
use std::collections::VecDeque;
use voice_loop::CAPTURE_RATE;
use voice_wire::Level;

/// Capture chunks of any size, cut into whole frames by `speech_vad::Framer`, in order. What is
/// left over waits for the next chunk; the end of the stream drops it. The frames wait here, so
/// clearing forgets them too: nothing heard before the wipe is stepped after it.
#[derive(Debug, Default)]
pub(crate) struct Frames {
    framer: Framer,
    ready: VecDeque<Frame512>,
    /// Samples pushed so far: where the next chunk starts.
    fed: u64,
}

impl Frames {
    /// Adds a chunk of capture.
    pub(crate) fn push(&mut self, samples: &[i16]) {
        let chunk = AudioChunk {
            format: AudioFormat {
                rate: CAPTURE_RATE,
                pcm: PcmFormat::S16Le,
            },
            at: SampleIndex(self.fed),
            pcm: PcmBytes::new(samples.iter().flat_map(|s| s.to_le_bytes()).collect()),
        };
        self.fed += samples.len() as u64;
        // A chunk in the capture format cannot be refused; one that is would add no frames.
        let framed = self.framer.push(&chunk).unwrap_or_default();
        self.ready.extend(framed.into_iter().map(|f| f.frame));
    }

    /// The next whole frame, if there is one.
    pub(crate) fn next_frame(&mut self) -> Option<Frame512> {
        self.ready.pop_front()
    }

    /// Forgets what is left over and every frame not yet taken (the utterance's audio is wiped).
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

/// The meter's level as the wire carries it. The one place the two `Level`s meet: `voice-wire`
/// has no stoker dependency, so the conversion lives here.
pub(crate) fn wire_level(level: speech_vad::Level) -> Level {
    Level(level.0)
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
        self.at += FRAME_SAMPLES as u64;
        matches!(self.state, Endpoint::Ended(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_sample(frame: &Frame512) -> i16 {
        frame.samples()[0]
    }

    #[test]
    fn chunks_are_cut_into_whole_frames_and_the_rest_waits() {
        let mut frames = Frames::default();
        frames.push(&[1; 700]);
        let first = frames.next_frame().expect("a frame");
        assert_eq!(first.samples().len(), FRAME_SAMPLES);
        assert!(frames.next_frame().is_none(), "188 samples wait");
        frames.push(&[2; 400]);
        let second = frames.next_frame().expect("another");
        assert_eq!(&second.samples()[..188], &[1; 188][..]);
        assert_eq!(second.samples()[188], 2);
        assert!(frames.next_frame().is_none());
    }

    #[test]
    fn the_capture_rate_is_the_rate_the_framer_takes() {
        let mut frames = Frames::default();
        frames.push(&[3; FRAME_SAMPLES]);
        assert_eq!(frames.next_frame().as_ref().map(first_sample), Some(3));
    }

    #[test]
    fn clearing_forgets_the_leftover_and_the_frames_not_yet_taken() {
        let mut frames = Frames::default();
        frames.push(&[0; 600]);
        frames.clear();
        frames.push(&[0; 400]);
        assert!(frames.next_frame().is_none());
        frames.push(&[0; 1200]);
        frames.clear();
        assert!(frames.next_frame().is_none());
    }

    #[test]
    fn the_meter_level_is_carried_over_unchanged() {
        assert_eq!(wire_level(speech_vad::Level(731)), Level(731));
    }

    #[test]
    fn a_dictation_that_hears_nothing_has_not_ended_after_one_frame() {
        let mut dictation = Dictation::new();
        let frame = Frame512::new(&[0; FRAME_SAMPLES]).expect("frame");
        assert!(!dictation.ended(&frame));
    }
}
