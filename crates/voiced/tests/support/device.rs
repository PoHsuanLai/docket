//! A device the test plays: the nodes it offers, the capture frames the test feeds, and a log of
//! what was opened and played. No wall clock: the capture stream is the clock, and it moves when
//! the test feeds a frame.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use voiced::{
    AudioDevice, AudioNode, CaptureFormat, CaptureStream, DeviceError, MediaClass, NodeId,
    NodeKind, PlaybackFormat, PlaybackStream,
};

/// What happened to the device.
#[derive(Debug, Default)]
pub struct Log {
    /// Captures ever opened.
    pub opened: AtomicUsize,
    /// Captures open now.
    pub open_now: AtomicUsize,
    /// Nodes captures were opened on.
    pub nodes: Mutex<Vec<NodeId>>,
    /// Everything played, with the rate of the stream it went to.
    pub played: Mutex<Vec<(u32, Vec<i16>)>>,
    /// The fades asked for, in milliseconds.
    pub faded: Mutex<Vec<u32>>,
    /// The formats captures were opened with.
    pub formats: Mutex<Vec<u32>>,
}

/// The test's hand on the device.
#[derive(Debug, Clone)]
pub struct Hand {
    pub log: Arc<Log>,
    feed: Arc<Mutex<Option<mpsc::UnboundedSender<Vec<i16>>>>>,
}

impl Hand {
    /// Sends one frame to the open capture; false if none is open.
    pub fn feed(&self, samples: Vec<i16>) -> usize {
        let feed = self.feed.lock().expect("feed");
        usize::from(feed.as_ref().is_some_and(|tx| tx.send(samples).is_ok()))
    }

    /// Ends the open capture stream from the device's side.
    pub fn end_stream(&self) {
        self.feed.lock().expect("feed").take();
    }

    pub fn open_now(&self) -> usize {
        self.log.open_now.load(Ordering::SeqCst)
    }

    pub fn opened(&self) -> usize {
        self.log.opened.load(Ordering::SeqCst)
    }
}

/// How the device answers an open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnOpen {
    Works,
    Fails(DeviceError),
}

#[derive(Debug, Clone)]
pub struct ScriptedDevice {
    nodes: Vec<AudioNode>,
    on_open: OnOpen,
    log: Arc<Log>,
    feed: Arc<Mutex<Option<mpsc::UnboundedSender<Vec<i16>>>>>,
}

pub fn node(id: u32, kind: NodeKind, class: &str) -> AudioNode {
    AudioNode {
        id: NodeId(id),
        name: format!("node-{id}"),
        kind,
        media_class: MediaClass(class.into()),
    }
}

impl ScriptedDevice {
    pub fn new(nodes: Vec<AudioNode>, on_open: OnOpen) -> (Self, Hand) {
        let log = Arc::new(Log::default());
        let feed = Arc::new(Mutex::new(None));
        let hand = Hand {
            log: log.clone(),
            feed: feed.clone(),
        };
        (
            Self {
                nodes,
                on_open,
                log,
                feed,
            },
            hand,
        )
    }

    /// A monitor and a microphone.
    pub fn with_a_microphone() -> (Self, Hand) {
        Self::new(
            vec![
                node(1, NodeKind::Monitor, "Audio/Source"),
                node(2, NodeKind::Source, "Audio/Source"),
            ],
            OnOpen::Works,
        )
    }
}

#[derive(Debug)]
pub struct ScriptedCapture {
    rx: mpsc::UnboundedReceiver<Vec<i16>>,
    log: Arc<Log>,
}

impl Drop for ScriptedCapture {
    fn drop(&mut self) {
        self.log.open_now.fetch_sub(1, Ordering::SeqCst);
    }
}

impl CaptureStream for ScriptedCapture {
    async fn next(&mut self) -> Option<Vec<i16>> {
        self.rx.recv().await
    }
}

#[derive(Debug)]
pub struct ScriptedPlayback {
    rate: u32,
    log: Arc<Log>,
}

impl PlaybackStream for ScriptedPlayback {
    async fn write(&mut self, samples: &[i16]) -> Result<(), DeviceError> {
        self.log
            .played
            .lock()
            .expect("played")
            .push((self.rate, samples.to_vec()));
        Ok(())
    }

    async fn fade_out(&mut self, ms: u32) {
        self.log.faded.lock().expect("faded").push(ms);
    }
}

impl AudioDevice for ScriptedDevice {
    type Capture = ScriptedCapture;
    type Playback = ScriptedPlayback;

    async fn sources(&self) -> Vec<AudioNode> {
        self.nodes.clone()
    }

    async fn open_capture(
        &self,
        node: &AudioNode,
        format: CaptureFormat,
    ) -> Result<ScriptedCapture, DeviceError> {
        if node.kind != NodeKind::Source {
            return Err(DeviceError::Denied);
        }
        if let OnOpen::Fails(error) = self.on_open {
            return Err(error);
        }
        let (tx, rx) = mpsc::unbounded_channel();
        *self.feed.lock().expect("feed") = Some(tx);
        self.log.opened.fetch_add(1, Ordering::SeqCst);
        self.log.open_now.fetch_add(1, Ordering::SeqCst);
        self.log.nodes.lock().expect("nodes").push(node.id);
        self.log.formats.lock().expect("formats").push(format.rate);
        Ok(ScriptedCapture {
            rx,
            log: self.log.clone(),
        })
    }

    async fn open_playback(&self, format: PlaybackFormat) -> Result<ScriptedPlayback, DeviceError> {
        Ok(ScriptedPlayback {
            rate: format.rate,
            log: self.log.clone(),
        })
    }
}
