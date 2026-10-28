//! Capture: a stream on one physical source node, run on a thread of its own, its frames handed
//! to the async side through a bounded queue.

use crate::format::format_pod;
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::spa::pod::Pod;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{Notify, oneshot};
use voice_audio::{CaptureStream, DeviceError, samples_of};

/// Capture buffers held for the async side before the oldest is dropped (about three seconds).
const CAPTURE_QUEUE: usize = 256;

/// Captured buffers waiting for the async side; a stalled reader loses the oldest, never memory.
#[derive(Debug, Default)]
pub(crate) struct FrameQueue {
    frames: Mutex<VecDeque<Vec<i16>>>,
    arrived: Notify,
    closed: AtomicBool,
}

impl FrameQueue {
    fn push(&self, samples: Vec<i16>) {
        if let Ok(mut frames) = self.frames.lock() {
            if frames.len() >= CAPTURE_QUEUE {
                frames.pop_front();
            }
            frames.push_back(samples);
        }
        self.arrived.notify_one();
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.arrived.notify_one();
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    fn pop(&self) -> Option<Vec<i16>> {
        self.frames.lock().ok().and_then(|mut f| f.pop_front())
    }
}

pub(crate) struct CaptureData {
    pub(crate) started: Option<oneshot::Sender<Result<(), DeviceError>>>,
    pub(crate) frames: Arc<FrameQueue>,
}

impl Drop for CaptureData {
    fn drop(&mut self) {
        self.frames.close();
    }
}

pub(crate) fn run_capture(
    target: String,
    rate: u32,
    data: CaptureData,
    quit: pw::channel::Receiver<()>,
) -> Result<(), pw::Error> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let latency = format!("512/{rate}");
    let mut props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Communication",
        *pw::keys::NODE_NAME => "voiced-capture",
        *pw::keys::NODE_LATENCY => latency.as_str(),
    };
    // `target.object`: the physical source by name (the key's constant needs a newer feature).
    props.insert("target.object", target.as_str());
    let stream = pw::stream::StreamBox::new(&core, "voiced-capture", props)?;
    let _listener = stream
        .add_local_listener_with_user_data(data)
        .state_changed(|_, data, _, new| {
            let verdict = match new {
                pw::stream::StreamState::Streaming => Some(Ok(())),
                pw::stream::StreamState::Error(why) => {
                    let lower = why.to_lowercase();
                    let denied = lower.contains("permission") || lower.contains("denied");
                    Some(Err(if denied {
                        DeviceError::Denied
                    } else {
                        DeviceError::NoSource
                    }))
                }
                _ => None,
            };
            if let (Some(verdict), Some(started)) = (verdict, data.started.take()) {
                let _ = started.send(verdict);
            }
        })
        .process(|stream, data| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(first) = buffer.datas_mut().first_mut() else {
                return;
            };
            let used = usize::try_from(first.chunk().size()).unwrap_or(0);
            let Some(bytes) = first.data() else { return };
            let samples = samples_of(bytes.get(..used.min(bytes.len())).unwrap_or_default());
            if !samples.is_empty() {
                data.frames.push(samples);
            }
        })
        .register()?;
    let values = format_pod(rate).ok_or(pw::Error::CreationFailed)?;
    let mut params = [Pod::from_bytes(&values).ok_or(pw::Error::CreationFailed)?];
    stream.connect(
        spa::utils::Direction::Input,
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

/// A running capture; dropping it closes the stream and so the microphone.
pub struct PipeWireCapture {
    pub(crate) frames: Arc<FrameQueue>,
    pub(crate) quit: pw::channel::Sender<()>,
}

impl std::fmt::Debug for PipeWireCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PipeWireCapture")
    }
}

impl Drop for PipeWireCapture {
    fn drop(&mut self) {
        let _ = self.quit.send(());
    }
}

impl CaptureStream for PipeWireCapture {
    async fn next(&mut self) -> Option<Vec<i16>> {
        loop {
            let arrival = self.frames.arrived.notified();
            tokio::pin!(arrival);
            arrival.as_mut().enable();
            if let Some(samples) = self.frames.pop() {
                return Some(samples);
            }
            if self.frames.is_closed() {
                return None;
            }
            arrival.await;
        }
    }
}
