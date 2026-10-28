//! One enumeration against an expiry.

use voice_audio::{
    AudioDevice, AudioNode, CaptureFormat, DeviceError, FakeAudioDevice, FakeCapture, FakePlayback,
    MediaClass, NodeId, NodeKind, PlaybackFormat, Snapshot, snapshot_before,
};

fn node(id: u32, kind: NodeKind, class: &str) -> AudioNode {
    AudioNode {
        id: NodeId(id),
        name: format!("n{id}"),
        kind,
        media_class: MediaClass(class.into()),
    }
}

/// A device whose enumeration never answers.
struct Silent(FakeAudioDevice);

impl AudioDevice for Silent {
    type Capture = FakeCapture;
    type Playback = FakePlayback;

    async fn snapshot(&self) -> Result<Snapshot, DeviceError> {
        std::future::pending().await
    }
    async fn sources(&self) -> Vec<AudioNode> {
        self.0.sources().await
    }
    async fn open_capture(
        &self,
        node: &AudioNode,
        format: CaptureFormat,
    ) -> Result<FakeCapture, DeviceError> {
        self.0.open_capture(node, format).await
    }
    async fn open_playback(&self, format: PlaybackFormat) -> Result<FakePlayback, DeviceError> {
        self.0.open_playback(format).await
    }
}

#[tokio::test]
async fn an_enumeration_that_never_answers_ends_in_a_timeout_when_the_budget_fires() {
    let device = Silent(FakeAudioDevice::default());
    let fired = snapshot_before(&device, std::future::ready(())).await;
    assert_eq!(fired, Err(DeviceError::TimedOut));
}

#[tokio::test]
async fn one_snapshot_carries_the_sources_and_the_default() {
    let device = FakeAudioDevice {
        nodes: vec![node(2, NodeKind::Source, "Audio/Source")],
        frames: Vec::new(),
    };
    let found = snapshot_before(&device, std::future::pending())
        .await
        .expect("answers");
    assert_eq!(found.sources, device.nodes);
    assert_eq!(found.default, None);
}
