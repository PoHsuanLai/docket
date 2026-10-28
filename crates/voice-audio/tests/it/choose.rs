//! The capture choice.

use voice_audio::{AudioNode, ChosenBy, MediaClass, NodeId, NodeKind, choose_capture};

fn node(id: u32, kind: NodeKind, class: &str) -> AudioNode {
    AudioNode {
        id: NodeId(id),
        name: format!("n{id}"),
        kind,
        media_class: MediaClass(class.into()),
    }
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
