//! The PipeWire backend of the device seam (spike V-A, FINDINGS.md). Capture asks PipeWire's
//! adapter for 16 kHz mono S16 from one physical `Audio/Source` node by name; a monitor or a sink
//! is refused before anything connects, so voiced cannot hear other apps. Each stream runs a main
//! loop on its own thread (PipeWire objects are not `Send`); frames cross to the async side over
//! a channel and the stream is closed when its handle drops.

use crate::device::{
    AudioDevice, AudioNode, CaptureFormat, CaptureStream, DeviceError, MediaClass, NodeId,
    NodeKind, PlaybackFormat, PlaybackStream,
};
use crate::playback::samples_of;
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::spa::pod::Pod;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Notify, mpsc, oneshot};

/// How long a stream may take to start before the device is called absent.
const START_TIMEOUT: Duration = Duration::from_secs(2);
/// Samples queued ahead of the playback device before `write` waits (two seconds at 24 kHz).
const PLAYBACK_AHEAD: usize = 48_000;

/// PipeWire, reached through its socket (`$XDG_RUNTIME_DIR/pipewire-0`).
#[derive(Debug, Clone, Copy, Default)]
pub struct PipeWireDevice;

fn format_pod(rate: u32) -> Option<Vec<u8>> {
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::S16LE);
    info.set_rate(rate);
    info.set_channels(1);
    let mut position = [0; spa::param::audio::MAX_CHANNELS];
    position[0] = spa::sys::SPA_AUDIO_CHANNEL_MONO;
    info.set_position(position);
    let object = spa::pod::Object {
        type_: spa::sys::SPA_TYPE_OBJECT_Format,
        id: spa::sys::SPA_PARAM_EnumFormat,
        properties: info.into(),
    };
    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )
    .ok()
    .map(|(cursor, _)| cursor.into_inner())
}

/// The node kind PipeWire's `media.class` and name say.
fn kind_of(class: &str, name: &str) -> Option<NodeKind> {
    match class {
        "Audio/Source" if name.ends_with(".monitor") => Some(NodeKind::Monitor),
        "Audio/Source" => Some(NodeKind::Source),
        "Audio/Source/Virtual" => Some(NodeKind::Monitor),
        "Audio/Sink" => Some(NodeKind::Sink),
        _ => None,
    }
}

fn list_nodes() -> Result<Vec<AudioNode>, pw::Error> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry()?;
    let found = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let done = std::rc::Rc::new(std::cell::Cell::new(false));
    let pending = core.sync(0)?;
    let (done_in, quit) = (done.clone(), mainloop.clone());
    let _core_listener = core
        .add_listener_local()
        .done(move |id, seq| {
            if id == pw::core::PW_ID_CORE && seq == pending {
                done_in.set(true);
                quit.quit();
            }
        })
        .register();
    let sink = found.clone();
    let _registry_listener = registry
        .add_listener_local()
        .global(move |global| {
            if global.type_ != pw::types::ObjectType::Node {
                return;
            }
            let Some(props) = global.props.as_ref() else {
                return;
            };
            let class = props.get("media.class").unwrap_or_default();
            let name = props.get("node.name").unwrap_or_default();
            if let Some(kind) = kind_of(class, name) {
                sink.borrow_mut().push(AudioNode {
                    id: NodeId(global.id),
                    name: name.to_owned(),
                    kind,
                    media_class: MediaClass(class.to_owned()),
                });
            }
        })
        .register();
    while !done.get() {
        mainloop.run();
    }
    let nodes = found.borrow().clone();
    Ok(nodes)
}

struct CaptureData {
    started: Option<oneshot::Sender<Result<(), DeviceError>>>,
    frames: mpsc::UnboundedSender<Vec<i16>>,
}

fn run_capture(
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
                let _ = data.frames.send(samples);
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
    frames: mpsc::UnboundedReceiver<Vec<i16>>,
    quit: pw::channel::Sender<()>,
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
        self.frames.recv().await
    }
}

#[derive(Debug, Default)]
struct Ring {
    samples: VecDeque<i16>,
    /// Samples still to fade over, and how many there were.
    fade: Option<(usize, usize)>,
}

#[derive(Debug, Default)]
struct PlayShared {
    ring: Mutex<Ring>,
    room: Notify,
    emptied: Notify,
}

fn fill(shared: &PlayShared, out: &mut [i16]) {
    let Ok(mut ring) = shared.ring.lock() else {
        out.fill(0);
        return;
    };
    for slot in out.iter_mut() {
        let sample = ring.samples.pop_front().unwrap_or(0);
        *slot = match ring.fade.as_mut() {
            Some((left, total)) => {
                let gain = i64::try_from(*left).unwrap_or(0);
                let whole = i64::try_from((*total).max(1)).unwrap_or(1);
                *left = left.saturating_sub(1);
                i16::try_from(i64::from(sample) * gain / whole).unwrap_or(0)
            }
            None => sample,
        };
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

fn run_playback(
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
                    let mut out = vec![0_i16; frames];
                    fill(shared, &mut out);
                    for (slot, sample) in bytes.chunks_exact_mut(stride).zip(out) {
                        slot.copy_from_slice(&sample.to_le_bytes());
                    }
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
    shared: Arc<PlayShared>,
    rate: u32,
    quit: pw::channel::Sender<()>,
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

async fn started(rx: oneshot::Receiver<Result<(), DeviceError>>) -> Result<(), DeviceError> {
    match tokio::time::timeout(START_TIMEOUT, rx).await {
        Ok(Ok(verdict)) => verdict,
        Ok(Err(_)) | Err(_) => Err(DeviceError::NoSource),
    }
}

impl AudioDevice for PipeWireDevice {
    type Capture = PipeWireCapture;
    type Playback = PipeWirePlayback;

    async fn sources(&self) -> Vec<AudioNode> {
        tokio::task::spawn_blocking(|| list_nodes().unwrap_or_default())
            .await
            .unwrap_or_default()
    }

    async fn open_capture(
        &self,
        node: &AudioNode,
        format: CaptureFormat,
    ) -> Result<PipeWireCapture, DeviceError> {
        if node.kind != NodeKind::Source {
            return Err(DeviceError::Denied);
        }
        let (frames_tx, frames) = mpsc::unbounded_channel();
        let (quit, quit_rx) = pw::channel::channel::<()>();
        let (started_tx, started_rx) = oneshot::channel();
        let (target, rate) = (node.name.clone(), format.rate);
        let data = CaptureData {
            started: Some(started_tx),
            frames: frames_tx,
        };
        std::thread::spawn(move || {
            let _ = run_capture(target, rate, data, quit_rx);
        });
        let capture = PipeWireCapture { frames, quit };
        started(started_rx).await?;
        Ok(capture)
    }

    async fn open_playback(&self, format: PlaybackFormat) -> Result<PipeWirePlayback, DeviceError> {
        let shared = Arc::new(PlayShared::default());
        let (quit, quit_rx) = pw::channel::channel::<()>();
        let (started_tx, started_rx) = oneshot::channel();
        let (rate, run_shared) = (format.rate, shared.clone());
        std::thread::spawn(move || {
            let _ = run_playback(rate, run_shared, started_tx, quit_rx);
        });
        let playback = PipeWirePlayback { shared, rate, quit };
        started(started_rx).await?;
        Ok(playback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_plain_source_is_a_microphone() {
        let table = [
            ("Audio/Source", "alsa_input.usb-mic", Some(NodeKind::Source)),
            (
                "Audio/Source",
                "alsa_output.pci.monitor",
                Some(NodeKind::Monitor),
            ),
            (
                "Audio/Source/Virtual",
                "echo-cancel-source",
                Some(NodeKind::Monitor),
            ),
            ("Audio/Sink", "alsa_output.pci", Some(NodeKind::Sink)),
            ("Stream/Output/Audio", "firefox", None),
            ("Video/Source", "camera", None),
        ];
        for (class, name, want) in table {
            assert_eq!(kind_of(class, name), want, "{class} {name}");
        }
    }

    #[test]
    fn a_fade_ends_in_silence_and_empties_the_ring() {
        let shared = PlayShared::default();
        {
            let mut ring = shared.ring.lock().expect("ring");
            ring.samples.extend(std::iter::repeat_n(1_000_i16, 100));
            ring.fade = Some((10, 10));
        }
        let mut out = [0_i16; 20];
        fill(&shared, &mut out);
        assert_eq!(out[0], 1_000);
        assert!(out[1] < out[0]);
        assert_eq!(&out[10..], &[0; 10]);
        assert!(shared.ring.lock().expect("ring").samples.is_empty());
    }
}
