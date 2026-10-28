//! The fake device.

use voice_audio::{
    AudioDevice, AudioNode, CaptureFormat, CaptureStream, DeviceError, FakeAudioDevice, MediaClass,
    NodeId, NodeKind, PlaybackFormat, PlaybackStream, choose_capture,
};

fn node(id: u32, kind: NodeKind, class: &str) -> AudioNode {
    AudioNode {
        id: NodeId(id),
        name: format!("n{id}"),
        kind,
        media_class: MediaClass(class.into()),
    }
}

#[tokio::test]
async fn the_fake_device_never_opens_a_monitor_and_plays_scripted_frames() {
    let device = FakeAudioDevice {
        nodes: vec![
            node(1, NodeKind::Monitor, "Audio/Source"),
            node(2, NodeKind::Source, "Audio/Source"),
        ],
        frames: vec![vec![1, 2, 3], vec![4]],
    };
    let nodes = device.sources().await;
    assert_eq!(
        device
            .open_capture(&nodes[0], CaptureFormat { rate: 16_000 })
            .await
            .map(|_| ()),
        Err(DeviceError::Denied)
    );
    let chosen = choose_capture(&nodes, None, None).expect("a source").node;
    let mut capture = device
        .open_capture(chosen, CaptureFormat { rate: 16_000 })
        .await
        .expect("capture");
    assert_eq!(capture.next().await, Some(vec![1, 2, 3]));
    assert_eq!(capture.next().await, Some(vec![4]));
    assert_eq!(capture.next().await, None);
    let mut out = device
        .open_playback(PlaybackFormat { rate: 24_000 })
        .await
        .expect("playback");
    out.write(&[0; 480]).await.expect("write");
    assert_eq!(out.written, 480);
}
