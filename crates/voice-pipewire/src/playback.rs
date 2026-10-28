//! Playback: a stream on the default output, run on a thread of its own, fed from a ring the async
//! side fills.

use crate::format::format_pod;
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::spa::pod::Pod;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::{Notify, oneshot};
use voice_audio::{DeviceError, PlaybackStream};

/// Samples queued ahead of the playback device before `write` waits (two seconds at 24 kHz).
const PLAYBACK_AHEAD: usize = 48_000;

#[derive(Debug, Default)]
struct Ring {
    samples: VecDeque<i16>,
    /// Samples still to fade over, and how many there were.
    fade: Option<(usize, usize)>,
}

#[derive(Debug, Default)]
pub(crate) struct PlayShared {
    ring: Mutex<Ring>,
    room: Notify,
    emptied: Notify,
}

/// Fills `out` (little-endian S16) from the ring in place; nothing is allocated.
fn fill(shared: &PlayShared, out: &mut [u8]) {
    let Ok(mut ring) = shared.ring.lock() else {
        out.fill(0);
        return;
    };
    for slot in out.as_chunks_mut::<2>().0 {
        let sample = ring.samples.pop_front().unwrap_or(0);
        let value = match ring.fade.as_mut() {
            Some((left, total)) => {
                let gain = i64::try_from(*left).unwrap_or(0);
                let whole = i64::try_from((*total).max(1)).unwrap_or(1);
                *left = left.saturating_sub(1);
                i16::try_from(i64::from(sample) * gain / whole).unwrap_or(0)
            }
            None => sample,
        };
        *slot = value.to_le_bytes();
    }
    if matches!(ring.fade, Some((0, _))) {
        ring.samples.clear();
    }
    let empty = ring.samples.is_empty();
    drop(ring);
    shared.room.notify_waiters();
    if empty {
        shared.emptied.notify_waiters();
    }
}

pub(crate) fn run_playback(
    rate: u32,
    shared: Arc<PlayShared>,
    started: oneshot::Sender<Result<(), DeviceError>>,
    quit: pw::channel::Receiver<()>,
) -> Result<(), pw::Error> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Playback",
        *pw::keys::MEDIA_ROLE => "Communication",
        *pw::keys::NODE_NAME => "voiced-playback",
    };
    let stream = pw::stream::StreamBox::new(&core, "voiced-playback", props)?;
    let _listener = stream
        .add_local_listener_with_user_data((shared, Some(started)))
        .state_changed(|_, (_, started), _, new| {
            let verdict = match new {
                pw::stream::StreamState::Streaming => Some(Ok(())),
                pw::stream::StreamState::Error(_) => Some(Err(DeviceError::NoSource)),
                _ => None,
            };
            if let (Some(verdict), Some(started)) = (verdict, started.take()) {
                let _ = started.send(verdict);
            }
        })
        .process(|stream, (shared, _)| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(first) = buffer.datas_mut().first_mut() else {
                return;
            };
            let stride = std::mem::size_of::<i16>();
            let frames = match first.data() {
                Some(bytes) => {
                    let frames = bytes.len() / stride;
                    fill(shared, bytes);
                    frames
                }
                None => 0,
            };
            let chunk = first.chunk_mut();
            *chunk.offset_mut() = 0;
            *chunk.stride_mut() = i32::try_from(stride).unwrap_or(2);
            *chunk.size_mut() = u32::try_from(stride * frames).unwrap_or(0);
        })
        .register()?;
    let values = format_pod(rate).ok_or(pw::Error::CreationFailed)?;
    let mut params = [Pod::from_bytes(&values).ok_or(pw::Error::CreationFailed)?];
    stream.connect(
        spa::utils::Direction::Output,
        None,
        pw::stream::StreamFlags::AUTOCONNECT
            | pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::DONT_RECONNECT,
        &mut params,
    )?;
    let stopper = mainloop.clone();
    let _attached = quit.attach(mainloop.loop_(), move |()| stopper.quit());
    mainloop.run();
    Ok(())
}

/// A running playback; dropping it closes the stream.
pub struct PipeWirePlayback {
    pub(crate) shared: Arc<PlayShared>,
    pub(crate) rate: u32,
    pub(crate) quit: pw::channel::Sender<()>,
}

impl std::fmt::Debug for PipeWirePlayback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PipeWirePlayback({} Hz)", self.rate)
    }
}

impl Drop for PipeWirePlayback {
    fn drop(&mut self) {
        let _ = self.quit.send(());
    }
}

impl PlaybackStream for PipeWirePlayback {
    async fn write(&mut self, samples: &[i16]) -> Result<(), DeviceError> {
        loop {
            let waiting = self.shared.room.notified();
            tokio::pin!(waiting);
            waiting.as_mut().enable();
            {
                let mut ring = self.shared.ring.lock().map_err(|_| DeviceError::Closed)?;
                if ring.samples.len() < PLAYBACK_AHEAD {
                    ring.samples.extend(samples.iter().copied());
                    return Ok(());
                }
            }
            waiting.await;
        }
    }

    async fn fade_out(&mut self, ms: u32) {
        let waiting = self.shared.emptied.notified();
        tokio::pin!(waiting);
        waiting.as_mut().enable();
        if ms > 0 {
            let total = usize::try_from(u64::from(ms) * u64::from(self.rate) / 1000).unwrap_or(1);
            if let Ok(mut ring) = self.shared.ring.lock() {
                ring.fade = Some((total.max(1), total.max(1)));
            }
        } else if self
            .shared
            .ring
            .lock()
            .is_ok_and(|ring| ring.samples.is_empty())
        {
            return;
        }
        waiting.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voice_audio::samples_of;

    #[test]
    fn a_fade_ends_in_silence_and_empties_the_ring() {
        let shared = PlayShared::default();
        {
            let mut ring = shared.ring.lock().expect("ring");
            ring.samples.extend(std::iter::repeat_n(1_000_i16, 100));
            ring.fade = Some((10, 10));
        }
        let mut bytes = [0_u8; 40];
        fill(&shared, &mut bytes);
        let out: Vec<i16> = samples_of(&bytes);
        assert_eq!(out[0], 1_000);
        assert!(out[1] < out[0]);
        assert_eq!(&out[10..], &[0; 10]);
        assert!(shared.ring.lock().expect("ring").samples.is_empty());
    }
}
