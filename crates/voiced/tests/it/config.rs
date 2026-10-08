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

fn named(id: u32, name: &str, kind: NodeKind, class: &str) -> AudioNode {
    AudioNode {
        name: name.into(),
        ..node(id, kind, class)
    }
}

fn pick(
    nodes: &[AudioNode],
    default: Option<&str>,
    input: Option<&str>,
) -> Option<(u32, ChosenBy)> {
    choose_capture(nodes, default, input).map(|c| (c.node.id.0, c.by))
}

/// What it is, the nodes, the default, the override, the expected id and reason.
type Row<'a> = (
    &'a str,
    &'a [AudioNode],
    Option<&'a str>,
    Option<&'a str>,
    Option<(u32, ChosenBy)>,
);

#[test]
fn the_capture_choice_table() {
    let line = named(1, "line", NodeKind::Source, "Audio/Source");
    let mic = named(2, "mic", NodeKind::Source, "Audio/Source");
    let monitor = named(3, "out.monitor", NodeKind::Monitor, "Audio/Source");
    let sink = named(4, "out", NodeKind::Sink, "Audio/Sink");
    let all = [monitor.clone(), sink.clone(), line.clone(), mic.clone()];
    let table: [Row; 13] = [
        ("no nodes", &[], None, None, None),
        (
            "no nodes, a default and an override",
            &[],
            Some("mic"),
            Some("mic"),
            None,
        ),
        (
            "only a monitor and a sink",
            &[monitor.clone(), sink.clone()],
            None,
            None,
            None,
        ),
        (
            "no default: the first physical",
            &all,
            None,
            None,
            Some((1, ChosenBy::First)),
        ),
        (
            "the default wins over order",
            &all,
            Some("mic"),
            None,
            Some((2, ChosenBy::Default)),
        ),
        (
            "a default naming a monitor is refused",
            &all,
            Some("out.monitor"),
            None,
            Some((1, ChosenBy::First)),
        ),
        (
            "a default naming a sink is refused",
            &all,
            Some("out"),
            None,
            Some((1, ChosenBy::First)),
        ),
        (
            "a default naming a missing node",
            &all,
            Some("gone"),
            None,
            Some((1, ChosenBy::First)),
        ),
        (
            "the override wins over the default",
            &all,
            Some("line"),
            Some("mic"),
            Some((2, ChosenBy::Override)),
        ),
        (
            "an override naming a missing node falls to the default",
            &all,
            Some("mic"),
            Some("gone"),
            Some((2, ChosenBy::Default)),
        ),
        (
            "an override naming a monitor is refused",
            &all,
            None,
            Some("out.monitor"),
            Some((1, ChosenBy::First)),
        ),
        (
            "monitor only plus a refused default",
            std::slice::from_ref(&monitor),
            Some("out.monitor"),
            None,
            None,
        ),
        (
            "a source whose class is not Audio/Source",
            &[named(5, "odd", NodeKind::Source, "Audio/Sink")],
            Some("odd"),
            Some("odd"),
            None,
        ),
    ];
    for (why, nodes, default, input, want) in table {
        assert_eq!(pick(nodes, default, input), want, "{why}");
    }
}

#[test]
fn the_input_override_is_optional_in_voiced_toml() {
    let base = "earcons = \"on\"\n";
    let roles = "[roles]\nshell = [\"org.quire.Shell\"]\n";
    let none = VoicedConfig::parse(&format!("{base}{roles}")).expect("parses");
    assert_eq!(none.input, None);
    let some = VoicedConfig::parse(&format!("{base}input = \"alsa_input.usb-mic\"\n{roles}"))
        .expect("parses");
    assert_eq!(some.input.as_deref(), Some("alsa_input.usb-mic"));
    let again = VoicedConfig::parse(&toml::to_string(&some).expect("toml")).expect("again");
    assert_eq!(again, some);
}

/// A device whose enumeration never answers.
#[cfg(feature = "testing")]
struct Silent(FakeAudioDevice);

#[cfg(feature = "testing")]
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

#[cfg(feature = "testing")]
#[tokio::test]
async fn an_enumeration_that_never_answers_ends_in_a_timeout_when_the_budget_fires() {
    let device = Silent(FakeAudioDevice::default());
    let fired = snapshot_before(&device, std::future::ready(())).await;
    assert_eq!(fired, Err(DeviceError::TimedOut));
}

#[cfg(feature = "testing")]
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
