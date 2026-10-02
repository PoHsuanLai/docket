//! The audio the daemon holds while the engine warms: at most ten seconds of 16 kHz mono S16,
//! oldest dropped first, zeroed when the utterance ends. Audio is never stored anywhere else.

use std::fmt;

/// Ten seconds at 16 kHz.
pub const BUFFER_SAMPLES: usize = 10 * 16_000;

/// What a push did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    /// Everything fit.
    Kept,
    /// The oldest samples were dropped to make room; the utterance is `TooLong`.
    DroppedOldest {
        /// How many samples were dropped.
        samples: usize,
    },
}

/// A ring of samples. No `Serialize`, no content in `Debug`.
pub struct PcmBuffer {
    slots: Vec<i16>,
    head: usize,
    len: usize,
}

impl PcmBuffer {
    /// An empty buffer of the full capacity.
    pub fn new() -> Self {
        Self {
            slots: vec![0; BUFFER_SAMPLES],
            head: 0,
            len: 0,
        }
    }

    /// How many samples it holds.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Appends samples, dropping the oldest when full.
    pub fn push(&mut self, samples: &[i16]) -> PushOutcome {
        let incoming = if samples.len() > BUFFER_SAMPLES {
            &samples[samples.len() - BUFFER_SAMPLES..]
        } else {
            samples
        };
        let mut dropped = samples.len() - incoming.len();
        for &s in incoming {
            if self.len == BUFFER_SAMPLES {
                self.head = (self.head + 1) % BUFFER_SAMPLES;
                self.len -= 1;
                dropped += 1;
            }
            let at = (self.head + self.len) % BUFFER_SAMPLES;
            self.slots[at] = s;
            self.len += 1;
        }
        if dropped == 0 {
            PushOutcome::Kept
        } else {
            PushOutcome::DroppedOldest { samples: dropped }
        }
    }

    /// Takes everything, oldest first, and wipes the buffer.
    pub fn take(&mut self) -> Vec<i16> {
        let out = (0..self.len)
            .map(|i| self.slots[(self.head + i) % BUFFER_SAMPLES])
            .collect();
        self.wipe();
        out
    }

    /// Overwrites every slot with zero and empties the buffer (the utterance ended).
    pub fn wipe(&mut self) {
        self.slots.fill(0);
        self.head = 0;
        self.len = 0;
    }

    /// Test hook: how many backing slots are not zero, held or not.
    pub fn nonzero_slots(&self) -> usize {
        self.slots.iter().filter(|s| **s != 0).count()
    }
}

impl Default for PcmBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for PcmBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PcmBuffer(<{} samples>)", self.len)
    }
}
