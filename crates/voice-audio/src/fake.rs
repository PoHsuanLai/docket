//! A scripted device for tests. Never opens a monitor.

use crate::device::{
    AudioDevice, CaptureFormat, CaptureStream, DeviceError, PlaybackFormat, PlaybackStream,
};
use crate::node::{AudioNode, NodeKind};

/// A device that plays scripted frames and records what was played, for tests.
#[derive(Debug, Clone, Default)]
pub struct FakeAudioDevice {
    /// The nodes it offers.
    pub nodes: Vec<AudioNode>,
    /// The frames capture yields.
    pub frames: Vec<Vec<i16>>,
}

/// The fake capture.
#[derive(Debug)]
pub struct FakeCapture {
    frames: std::collections::VecDeque<Vec<i16>>,
}

/// The fake playback; it counts samples and drops them.
#[derive(Debug, Default)]
pub struct FakePlayback {
    /// How many samples were written.
    pub written: usize,
}

impl CaptureStream for FakeCapture {
    async fn next(&mut self) -> Option<Vec<i16>> {
        self.frames.pop_front()
    }
}

impl PlaybackStream for FakePlayback {
    async fn write(&mut self, samples: &[i16]) -> Result<(), DeviceError> {
        self.written += samples.len();
        Ok(())
    }
    async fn fade_out(&mut self, _ms: u32) {}
}

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
