//! The configuration, the capture choice and the fake device.

use porter_core::AppName;
use std::path::PathBuf;
use voiced::*;

fn node(id: u32, kind: NodeKind, class: &str) -> AudioNode {
    AudioNode {
        id: NodeId(id),
        name: format!("n{id}"),
        kind,
        media_class: MediaClass(class.into()),
    }
}

#[test]
fn monitor_source_refused() {
    // A monitor is never chosen, even when it claims to be a source and comes first.
    let nodes = [
        node(1, NodeKind::Monitor, "Audio/Source"),
        node(2, NodeKind::Sink, "Audio/Sink"),
        node(3, NodeKind::Source, "Audio/Source"),
    ];
    assert_eq!(choose_capture(&nodes).map(|n| n.id), Some(NodeId(3)));
    assert_eq!(choose_capture(&nodes[..2]), None);
    assert_eq!(choose_capture(&[]), None);
    assert_eq!(
        choose_capture(&[node(4, NodeKind::Source, "Audio/Sink")]),
        None
    );
}

#[test]
fn the_shipped_voiced_toml_parses_and_round_trips() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/voiced.toml");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let config = VoicedConfig::parse(&text).expect("parses");
    let shell = AppName::parse("org.quire.Shell").expect("app");
    assert_eq!(config.role_of(&shell), Some(VoiceRole::Shell));
    assert_eq!(
        config.role_of(&AppName::parse("org.quire.Mail").expect("app")),
        None
    );
    let again = VoicedConfig::parse(&toml::to_string(&config).expect("toml")).expect("again");
    assert_eq!(again, config);
}

#[test]
fn a_file_without_a_shell_is_refused() {
    assert_eq!(
        VoicedConfig::parse("earcons = \"on\"\n[roles]\n"),
        Err(ConfigError::NoShell)
    );
    assert!(matches!(
        VoicedConfig::parse("nonsense"),
        Err(ConfigError::Parse(_))
    ));
}

#[cfg(feature = "testing")]
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
    let chosen = choose_capture(&nodes).expect("a source");
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
