//! The audio device seam. Capture reads only a physical source node, never a sink monitor, so
//! voiced cannot transcribe other apps' audio.

use serde::{Deserialize, Serialize};
use std::future::Future;

/// A PipeWire node id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub u32);

/// What a node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// A physical input (a microphone).
    Source,
    /// The monitor of an output: other apps' audio. Never captured.
    Monitor,
    /// An output.
    Sink,
}

/// The node's `media.class` property (`Audio/Source`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MediaClass(pub String);

/// One audio node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioNode {
    /// Its id.
    pub id: NodeId,
    /// Its name.
    pub name: String,
    /// What it is.
    pub kind: NodeKind,
    /// Its `media.class`.
    pub media_class: MediaClass,
}

/// The first physical source. A monitor is never returned, whatever its class says.
pub fn choose_capture(nodes: &[AudioNode]) -> Option<&AudioNode> {
    nodes
        .iter()
        .find(|n| n.kind == NodeKind::Source && n.media_class.0 == "Audio/Source")
}

/// The capture format: 16 kHz mono S16, converted by PipeWire's adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureFormat {
    /// Samples per second.
    pub rate: u32,
}

/// The playback format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaybackFormat {
    /// Samples per second (the TTS engine's own).
    pub rate: u32,
}

/// Why the device failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DeviceError {
    /// No physical source.
    #[error("no microphone")]
    NoSource,
    /// The system denied it.
    #[error("denied")]
    Denied,
    /// The stream ended.
    #[error("stream closed")]
    Closed,
}

/// A running capture.
pub trait CaptureStream: Send + Sync {
    /// The next frame of samples, or none when the stream ended.
    fn next(&mut self) -> impl Future<Output = Option<Vec<i16>>> + Send;
}

/// A running playback.
pub trait PlaybackStream: Send {
    /// Plays samples.
    fn write(&mut self, samples: &[i16]) -> impl Future<Output = Result<(), DeviceError>> + Send;
    /// Fades out over `ms` and drains.
    fn fade_out(&mut self, ms: u32) -> impl Future<Output = ()> + Send;
}

/// The device: PipeWire in the shipped build, a fake in tests.
pub trait AudioDevice: Send + Sync {
    /// The capture stream type.
    type Capture: CaptureStream;
    /// The playback stream type.
    type Playback: PlaybackStream;

    /// The nodes now present.
    fn sources(&self) -> impl Future<Output = Vec<AudioNode>> + Send;
    /// Opens capture on a node (the caller chose it with [`choose_capture`]).
    fn open_capture(
        &self,
        node: &AudioNode,
        format: CaptureFormat,
    ) -> impl Future<Output = Result<Self::Capture, DeviceError>> + Send;
    /// Opens playback on the default output.
    fn open_playback(
        &self,
        format: PlaybackFormat,
    ) -> impl Future<Output = Result<Self::Playback, DeviceError>> + Send;
}

/// A device that plays scripted frames and records what was played, for tests.
#[cfg(feature = "testing")]
#[derive(Debug, Clone, Default)]
pub struct FakeAudioDevice {
    /// The nodes it offers.
    pub nodes: Vec<AudioNode>,
    /// The frames capture yields.
    pub frames: Vec<Vec<i16>>,
}

/// The fake capture.
#[cfg(feature = "testing")]
#[derive(Debug)]
pub struct FakeCapture {
    frames: std::collections::VecDeque<Vec<i16>>,
}

/// The fake playback; it counts samples and drops them.
#[cfg(feature = "testing")]
#[derive(Debug, Default)]
pub struct FakePlayback {
    /// How many samples were written.
    pub written: usize,
}

#[cfg(feature = "testing")]
impl CaptureStream for FakeCapture {
    async fn next(&mut self) -> Option<Vec<i16>> {
        self.frames.pop_front()
    }
}

#[cfg(feature = "testing")]
impl PlaybackStream for FakePlayback {
    async fn write(&mut self, samples: &[i16]) -> Result<(), DeviceError> {
        self.written += samples.len();
        Ok(())
    }
    async fn fade_out(&mut self, _ms: u32) {}
}

#[cfg(feature = "testing")]
impl AudioDevice for FakeAudioDevice {
    type Capture = FakeCapture;
    type Playback = FakePlayback;

    async fn sources(&self) -> Vec<AudioNode> {
        self.nodes.clone()
    }

    async fn open_capture(
        &self,
        node: &AudioNode,
        _format: CaptureFormat,
    ) -> Result<FakeCapture, DeviceError> {
        if node.kind == NodeKind::Monitor {
            return Err(DeviceError::Denied);
        }
        Ok(FakeCapture {
            frames: self.frames.iter().cloned().collect(),
        })
    }

    async fn open_playback(&self, _format: PlaybackFormat) -> Result<FakePlayback, DeviceError> {
        Ok(FakePlayback::default())
    }
}
