//! Restore-point data: serde shapes, `WorkPath` rules, the `Rewind` default.

use docket_core::*;
use porter_core::{Count, UnixSeconds};
use prov::SessionId;
use serde::{Serialize, de::DeserializeOwned};

fn round<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: &T) -> String {
    let json = serde_json::to_string(value).expect("json");
    let back: T = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{json}: {e}"));
    assert_eq!(&back, value, "{json}");
    json
}

fn path(text: &str) -> WorkPath {
    WorkPath::parse(text).expect("path")
}

#[test]
fn every_checkpoint_type_round_trips_in_its_pinned_shape() {
    let at = UnixSeconds(1_700_000_000);
    let turn = TurnId(4);
    let rows: [(&str, String); 8] = [
        ("id", round(&CheckpointId(3))),
        ("path", round(&path("src/a b/c.rs"))),
        ("rewind", round(&Rewind::Agent)),
        ("skip", round(&SkipReason::AgentKeepsOwn)),
        ("taken", round(&CheckpointEvent::Taken(CheckpointId(2)))),
        (
            "restored",
            round(&CheckpointEvent::Restored {
                to: CheckpointId(1),
                safety: CheckpointId(5),
            }),
        ),
        (
            "note",
            round(&CheckpointNote {
                turn,
                at,
                event: CheckpointEvent::Skipped(SkipReason::NoHistory),
            }),
        ),
        ("state", round(&SavedState::Gone)),
    ];
    let want = [
        ("id", "3"),
        ("path", r#""src/a b/c.rs""#),
        ("rewind", r#""agent""#),
        ("skip", r#""agent_keeps_own""#),
        ("taken", r#"{"kind":"taken","v":2}"#),
        ("restored", r#"{"kind":"restored","v":{"to":1,"safety":5}}"#),
        (
            "note",
            r#"{"turn":4,"at":1700000000,"event":{"kind":"skipped","v":"no_history"}}"#,
        ),
        ("state", r#""gone""#),
    ];
    for ((name, got), (_, expected)) in rows.iter().zip(want) {
        assert_eq!(got, expected, "{name}");
    }
    let list = CheckpointList {
        session: SessionId::parse("s-1").expect("session"),
        rows: vec![
            CheckpointRow::Saved {
                id: CheckpointId(1),
                turn,
                at,
                state: SavedState::Available,
            },
            CheckpointRow::NotSaved {
                turn,
                at,
                why: SkipReason::TooLarge,
            },
            CheckpointRow::RestoredTo {
                id: CheckpointId(1),
                at,
            },
        ],
        keeps: Retention {
            last: Count(20),
            days: Days(14),
        },
    };
    round(&list);
    round(&RestorePlan {
        changed: vec![path("a")],
        added: vec![path("b")],
        removed: vec![path("c/d")],
        digest: PlanDigest("00".into()),
    });
    round(&CheckpointFault::PlanStale);
}

#[test]
fn a_work_path_refuses_what_could_leave_the_workspace() {
    let rows: [(&str, Result<(), WorkPathError>); 11] = [
        ("a", Ok(())),
        ("a/b.txt", Ok(())),
        (".hidden/x", Ok(())),
        ("a..b", Ok(())),
        ("", Err(WorkPathError::Empty)),
        ("/etc/passwd", Err(WorkPathError::Absolute)),
        ("../x", Err(WorkPathError::Step)),
        ("a/../b", Err(WorkPathError::Step)),
        ("a//b", Err(WorkPathError::Step)),
        ("./a", Err(WorkPathError::Step)),
        ("a\0b", Err(WorkPathError::Nul)),
    ];
    for (text, want) in rows {
        assert_eq!(WorkPath::parse(text).map(|_| ()), want, "{text:?}");
    }
    // The wire checks the same rules.
    assert!(serde_json::from_str::<WorkPath>(r#""../x""#).is_err());
}

#[test]
fn an_old_external_agent_record_reads_as_docket_keeping_the_history() {
    let old = r#"{"program":"claude-code","sheets":"desktop"}"#;
    let agent: ExternalAgent = serde_json::from_str(old).expect("old record");
    assert_eq!(agent.rewind, Rewind::Docket);
    assert_eq!(Rewind::default(), Rewind::Docket);
}

#[test]
fn the_checkpoint_settings_rows_carry_the_defaults() {
    let config = AgentConfig::default();
    for (key, want) in [
        ("agent.checkpoints.keep", 20),
        ("agent.checkpoints.days", 14),
        ("agent.checkpoints.max_files", 50_000),
        ("agent.checkpoints.wait_s", 10),
    ] {
        assert_eq!(config.value(key), Some(SettingValue::Number(want)), "{key}");
    }
}
