//! Speech and earcon playback. The speech player is a task that owns the playback stream, so a
//! slow device never stops the loop and a barge-in can cut what is queued: every command carries
//! the generation it was sent in, and a stop makes the older ones stale. Half-duplex is the
//! machines' business (speech never plays while the mic is open); this file only plays.

use crate::device::{AudioDevice, PlaybackFormat, PlaybackStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc;
use voice_loop::Earcon;

/// The rate earcons play at.
pub const EARCON_RATE: u32 = 24_000;

#[derive(Debug)]
enum Cmd {
    Samples {
        generation: u64,
        rate: u32,
        data: Vec<i16>,
    },
    Fade {
        generation: u64,
        ms: u32,
    },
    Drain {
        generation: u64,
    },
}

/// What the task reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Played {
    /// What was queued in this generation has been played out (or faded out).
    Drained(u64),
    /// The device refused the samples.
    Failed,
}

/// The loop's end of the playback task.
#[derive(Debug)]
pub(crate) struct Player {
    tx: mpsc::UnboundedSender<Cmd>,
    current: Arc<AtomicU64>,
}

impl Player {
    /// Starts the task over `device`; it reports on `events`.
    pub fn spawn<D: AudioDevice + 'static>(
        device: Arc<D>,
        events: mpsc::UnboundedSender<Played>,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let current = Arc::new(AtomicU64::new(0));
        tokio::spawn(run(device, rx, current.clone(), events));
        Self { tx, current }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.current.load(Ordering::SeqCst)
    }

    /// Queues samples at `rate`.
    pub fn play(&self, rate: u32, data: Vec<i16>) {
        let generation = self.generation();
        let _ = self.tx.send(Cmd::Samples {
            generation,
            rate,
            data,
        });
    }

    /// Reports `Drained` when everything queued has played.
    pub fn drain(&self) {
        let generation = self.generation();
        let _ = self.tx.send(Cmd::Drain { generation });
    }

    /// Drops what is queued, fades out over `ms` and reports `Drained`.
    pub fn stop(&self, ms: u32) {
        let generation = self.current.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.tx.send(Cmd::Fade { generation, ms });
    }
}

async fn run<D: AudioDevice>(
    device: Arc<D>,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
    current: Arc<AtomicU64>,
    events: mpsc::UnboundedSender<Played>,
) {
    let mut open: Option<(u32, D::Playback)> = None;
    while let Some(cmd) = rx.recv().await {
        match cmd {
            Cmd::Samples {
                generation,
                rate,
                data,
            } if generation == current.load(Ordering::SeqCst) => {
                if open.as_ref().is_none_or(|(r, _)| *r != rate) {
                    open = device
                        .open_playback(PlaybackFormat { rate })
                        .await
                        .ok()
                        .map(|stream| (rate, stream));
                }
                let written = match open.as_mut() {
                    Some((_, stream)) => stream.write(&data).await.is_ok(),
                    None => false,
                };
                if !written {
                    open = None;
                    let _ = events.send(Played::Failed);
                }
            }
            Cmd::Samples { .. } => {}
            Cmd::Fade { generation, ms } => {
                if let Some((_, stream)) = open.as_mut() {
                    stream.fade_out(ms).await;
                }
                open = None;
                let _ = events.send(Played::Drained(generation));
            }
            Cmd::Drain { generation } if generation == current.load(Ordering::SeqCst) => {
                if let Some((_, stream)) = open.as_mut() {
                    stream.fade_out(0).await;
                }
                open = None;
                let _ = events.send(Played::Drained(generation));
            }
            Cmd::Drain { .. } => {}
        }
    }
}

/// S16LE bytes as samples (a trailing odd byte is dropped).
pub(crate) fn samples_of(bytes: &[u8]) -> Vec<i16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect()
}

/// A short two-note blip: rising for begin, falling for end. A triangle wave in integers, with a
/// linear fade so it does not click.
pub fn earcon_samples(earcon: Earcon) -> Vec<i16> {
    let (first, second) = match earcon {
        Earcon::Begin => (30_u32, 24_u32),
        Earcon::End => (24, 30),
    };
    let note = |period: u32, count: u32| {
        (0..count).map(move |i| {
            let phase = i32::try_from(i % period).unwrap_or(0);
            let half = i32::try_from(period / 2).unwrap_or(1);
            let tri = if phase < half {
                phase
            } else {
                i32::try_from(period).unwrap_or(0) - phase
            };
            let wave = (tri * 2 - half) * 2_400 / half;
            let left = i32::try_from(count - i).unwrap_or(0);
            let fade = left.min(240);
            i16::try_from(wave * fade / 240).unwrap_or(0)
        })
    };
    note(first, 1_200).chain(note(second, 1_200)).collect()
}

/// Plays one earcon on its own stream; a device that cannot is silent, never an error.
pub(crate) fn earcon<D: AudioDevice + 'static>(device: Arc<D>, which: Earcon) {
    tokio::spawn(async move {
        let Ok(mut stream) = device
            .open_playback(PlaybackFormat { rate: EARCON_RATE })
            .await
        else {
            return;
        };
        if stream.write(&earcon_samples(which)).await.is_ok() {
            stream.fade_out(0).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_earcon_is_short_quiet_and_ends_in_silence() {
        for which in [Earcon::Begin, Earcon::End] {
            let samples = earcon_samples(which);
            assert_eq!(samples.len(), 2_400);
            assert!(samples.iter().all(|s| s.unsigned_abs() <= 2_400));
            assert!(samples.iter().any(|s| s.unsigned_abs() > 1_000));
            assert!(samples.last().is_some_and(|s| s.unsigned_abs() < 40));
        }
        assert_ne!(earcon_samples(Earcon::Begin), earcon_samples(Earcon::End));
    }
}
