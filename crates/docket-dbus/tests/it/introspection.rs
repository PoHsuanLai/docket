//! The checked-in introspection files are the interfaces the skeletons declare. A change to a
//! signature changes the file in the same commit.

use docket_dbus::{Bus, OPTION_TRACEPARENT, introspection};
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dbus")
}

#[test]
fn checked_in_introspection_matches_the_interfaces() {
    for bus in Bus::ALL {
        let path = dir().join(bus.file_name());
        let expected =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let actual = introspection(bus);
        assert!(
            actual == expected,
            "{} differs from the interfaces; it should read:\n{actual}",
            path.display()
        );
    }
}

fn members(xml: &str) -> usize {
    xml.matches("<method ").count()
        + xml.matches("<signal ").count()
        + xml.matches("<property ").count()
}

#[test]
fn every_intents_member_is_declared() {
    let xml = introspection(Bus::Intents);
    let want = [
        "interface name=\"org.quire.Intents1.Registry\"",
        "<method name=\"Manifests\">",
        "<signal name=\"ManifestChanged\">",
        "interface name=\"org.quire.Intents1.Index\"",
        "<method name=\"Push\">",
        "<method name=\"Reset\">",
        "interface name=\"org.quire.Intents1.Search\"",
        "<method name=\"Query\">",
        "<method name=\"Cancel\">",
        "<signal name=\"Hits\">",
        "interface name=\"org.quire.Intents1.Run\"",
        "<method name=\"Perform\">",
        "<method name=\"Preview\">",
        "<method name=\"Suggest\">",
        "<method name=\"Undo\">",
        "<method name=\"UndoAll\">",
        "interface name=\"org.quire.Intents1.Context\"",
        "<method name=\"Current\">",
        "interface name=\"org.quire.Intents1.Session\"",
        "<method name=\"Open\">",
        "<method name=\"Turn\">",
        "<method name=\"Close\">",
        "<method name=\"Resolve\">",
        "<method name=\"Display\">",
        "<method name=\"DisplayLabelled\">",
        "<method name=\"Read\">",
        "<method name=\"TaskPolicy\">",
        "<method name=\"TurnEnded\">",
        "<method name=\"Widen\">",
        "<method name=\"Note\">",
        "<method name=\"Recall\">",
        "interface name=\"org.quire.Intents1.Checkpoint\"",
        "<method name=\"List\">",
        "<method name=\"Plan\">",
        "<method name=\"Watch\">",
        "<method name=\"Mark\">",
        "interface name=\"org.quire.Intents1.Message\"",
        "<method name=\"Send\">",
        "<method name=\"Inbox\">",
        "<signal name=\"Arrived\">",
        "interface name=\"org.quire.Intents1.Gate\"",
        "<method name=\"Grant\">",
        "<method name=\"Check\">",
        "interface name=\"org.quire.Intents1.Control\"",
        "<method name=\"Halt\">",
        "<method name=\"Resume\">",
        "<method name=\"State\">",
        "<method name=\"Journal\">",
        "<method name=\"TerminalGrants\">",
        "<method name=\"RevokeTerminalGrant\">",
        "<method name=\"DryRun\">",
        "<signal name=\"Halted\">",
        "<signal name=\"Resumed\">",
        "<signal name=\"JournalChanged\">",
        "<signal name=\"BreakerTripped\">",
        "interface name=\"org.quire.Intents1.Request\"",
        "<method name=\"Proceed\">",
        "<signal name=\"Progress\">",
        "<signal name=\"Response\">",
    ];
    for w in want {
        assert!(xml.contains(w), "missing {w}");
    }
    // One method per `Member` of docket-core, plus the Request object's Close and Proceed.
    assert_eq!(
        xml.matches("<method ").count(),
        docket_core::Member::ALL.len() + 2
    );
}

#[test]
fn provider_confirm_companion_and_reader_members() {
    let cases: [(Bus, &[&str]); 4] = [
        (
            Bus::IntentProvider,
            &[
                "<method name=\"Perform\">",
                "<method name=\"DryRun\">",
                "<method name=\"Undo\">",
                "<method name=\"Context\">",
                "<method name=\"Summon\">",
                "<method name=\"Search\">",
                "<method name=\"Preview\">",
                "<method name=\"Suggest\">",
                "<method name=\"Resolve\">",
                "<signal name=\"UndoChanged\">",
                "<signal name=\"IndexStale\">",
            ],
        ),
        (
            Bus::Confirm,
            &["<method name=\"Confirm\">", "<method name=\"Cancel\">"],
        ),
        (
            Bus::Companion,
            &[
                "<method name=\"Prepare\">",
                "<method name=\"Open\">",
                "<method name=\"Ask\">",
                "<method name=\"Close\">",
                "<method name=\"Roster\">",
                "<method name=\"Front\">",
                "<signal name=\"AnswerAdded\">",
                "<signal name=\"AnswerRemoved\">",
                "<signal name=\"RosterChanged\">",
                "<method name=\"Act\">",
                "<signal name=\"Updated\">",
                "<property name=\"View\" type=\"s\" access=\"read\"/>",
            ],
        ),
        (Bus::Reader, &["<method name=\"Extract\">"]),
    ];
    for (bus, want) in cases {
        let xml = introspection(bus);
        for w in want {
            assert!(xml.contains(w), "{bus:?} is missing {w}");
        }
    }
    assert_eq!(members(&introspection(Bus::IntentProvider)), 12);
    assert_eq!(members(&introspection(Bus::Confirm)), 2);
    assert_eq!(members(&introspection(Bus::Reader)), 1);
}

#[test]
fn companion1_has_no_presence_undo_activity_or_pause_members() {
    let xml = introspection(Bus::Companion);
    for word in ["Presence", "Undo", "Activity", "PauseSpace"] {
        assert!(
            !xml.contains(word),
            "Companion1 must not declare {word}: sill derives presence, the journal is Intents1's"
        );
    }
}

#[test]
fn calls_that_start_work_carry_an_options_dict_for_the_reserved_traceparent() {
    let intents = introspection(Bus::Intents);
    let options = "<arg name=\"options\" type=\"a{sv}\" direction=\"in\"/>";
    // Run.Perform, Session.Turn, Session.Read, Session.Recall, Message.Send, Gate.Check.
    assert_eq!(intents.matches(options).count(), 6);
    assert_eq!(
        introspection(Bus::Companion).matches(options).count(),
        2,
        "Prepare and Ask"
    );
    assert_eq!(introspection(Bus::Reader).matches(options).count(), 1);
    assert_eq!(
        introspection(Bus::IntentProvider).matches(options).count(),
        3,
        "Perform, Classify and DryRun"
    );
    assert_eq!(OPTION_TRACEPARENT, "traceparent");
}

#[test]
fn signals_that_everyone_can_hear_carry_no_content() {
    // A broadcast must tell that something happened, never what: receivers read through a call
    // their role allows.
    let message = introspection(Bus::Intents);
    let arrived = message
        .split("<signal name=\"Arrived\">")
        .nth(1)
        .and_then(|s| s.split("</signal>").next())
        .expect("Arrived");
    assert_eq!(arrived.matches("<arg ").count(), 1, "only the addressee");
    let roster = introspection(Bus::Companion);
    let changed = roster
        .split("<signal name=\"RosterChanged\">")
        .nth(1)
        .and_then(|s| s.split("</signal>").next())
        .expect("RosterChanged");
    assert_eq!(changed.matches("<arg ").count(), 0);
}
