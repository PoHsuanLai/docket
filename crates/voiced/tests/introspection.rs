//! The checked-in introspection is the interface the skeletons declare.

use std::path::PathBuf;
use voiced::{VOICE1_FILE, introspection};

fn checked_in() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../dbus")
        .join(VOICE1_FILE);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn voice1_introspection_matches_xml() {
    let actual = introspection();
    assert!(
        actual == checked_in(),
        "dbus/{VOICE1_FILE} differs from the interfaces; it should read:\n{actual}"
    );
}

#[test]
fn every_member_is_declared_and_no_more() {
    let xml = introspection();
    let members = [
        "<interface name=\"org.quire.Voice1\">",
        "<interface name=\"org.quire.Voice1.Utterance\">",
        "<interface name=\"org.quire.Voice1.Speech\">",
        "<method name=\"Begin\">",
        "<method name=\"Speak\">",
        "<method name=\"Hush\">",
        "<method name=\"Prepare\">",
        "<method name=\"Status\">",
        "<signal name=\"StatusChanged\">",
        "<method name=\"Route\">",
        "<method name=\"Attach\">",
        "<method name=\"Release\">",
        "<method name=\"Cancel\">",
        "<signal name=\"Ended\">",
        "<method name=\"Stop\">",
        "<signal name=\"Finished\">",
    ];
    for m in members {
        assert!(xml.contains(m), "missing {m}");
    }
    let declared = xml.matches("<method ").count() + xml.matches("<signal ").count();
    assert_eq!(declared, 13);
    assert!(
        xml.contains(
            "<method name=\"Cancel\">\n     <arg name=\"cause\" type=\"s\" direction=\"in\"/>"
        ),
        "Cancel takes a sealed cause"
    );
    assert!(
        xml.contains("<arg type=\"h\" direction=\"out\"/>"),
        "the event stream is an fd"
    );
}

#[test]
fn no_member_carries_audio_or_text_bodies() {
    let xml = introspection();
    for word in ["audio", "Audio", "pcm", "transcript", "Transcript", "text"] {
        assert!(
            !xml.contains(word),
            "{word} must never be a member or argument"
        );
    }
    // Arguments are JSON strings, object paths and fds only.
    for arg in xml.lines().filter(|l| l.contains("<arg ")) {
        assert!(
            ["type=\"s\"", "type=\"o\"", "type=\"h\""]
                .iter()
                .any(|t| arg.contains(t)),
            "{arg}"
        );
    }
}
