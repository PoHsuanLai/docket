//! The device itself: one snapshot per enumeration, streams opened on demand.

use crate::capture::{CaptureData, FrameQueue, PipeWireCapture, run_capture};
use crate::nodes::list_nodes;
use crate::playback::{PipeWirePlayback, PlayShared, run_playback};
use pipewire as pw;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use voice_audio::{
    AudioDevice, AudioNode, CaptureFormat, DeviceError, NodeKind, PlaybackFormat, SNAPSHOT_BUDGET,
    Snapshot,
};

/// How long a stream may take to start before the device is called absent.
const START_TIMEOUT: Duration = Duration::from_secs(2);

/// PipeWire, reached through its socket (`$XDG_RUNTIME_DIR/pipewire-0`).
#[derive(Debug, Clone, Copy, Default)]
pub struct PipeWireDevice;

async fn started(rx: oneshot::Receiver<Result<(), DeviceError>>) -> Result<(), DeviceError> {
    match tokio::time::timeout(START_TIMEOUT, rx).await {
        Ok(Ok(verdict)) => verdict,
        Ok(Err(_)) | Err(_) => Err(DeviceError::NoSource),
    }
}

impl AudioDevice for PipeWireDevice {
    type Capture = PipeWireCapture;
    type Playback = PipeWirePlayback;

    async fn snapshot(&self) -> Result<Snapshot, DeviceError> {
        let listed = tokio::task::spawn_blocking(|| list_nodes(SNAPSHOT_BUDGET))
            .await
            .map_err(|_| DeviceError::Closed)?;
        let (sources, defaults) = listed.map_err(|_| DeviceError::TimedOut)?;
        Ok(Snapshot {
            sources,
            default: defaults.name().map(str::to_owned),
        })
    }

    async fn sources(&self) -> Vec<AudioNode> {
        self.snapshot().await.map(|s| s.sources).unwrap_or_default()
    }

    async fn default_source(&self) -> Option<String> {
        self.snapshot().await.ok().and_then(|s| s.default)
    }

    async fn open_capture(
        &self,
        node: &AudioNode,
        format: CaptureFormat,
    ) -> Result<PipeWireCapture, DeviceError> {
        if node.kind != NodeKind::Source {
            return Err(DeviceError::Denied);
        }
        let frames = Arc::new(FrameQueue::default());
        let (quit, quit_rx) = pw::channel::channel::<()>();
        let (started_tx, started_rx) = oneshot::channel();
        let (target, rate) = (node.name.clone(), format.rate);
        let data = CaptureData {
            started: Some(started_tx),
            frames: frames.clone(),
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
