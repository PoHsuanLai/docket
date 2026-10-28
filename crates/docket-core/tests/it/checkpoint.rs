//! Restore-point data: serde shapes, `WorkPath` rules, the `Rewind` default.

use docket_core::*;
use docket_permissions_core::ProgramName;
use docket_vocab::{Days, TurnId, Workspace};
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
        turn: TurnState::Running,
        label: Some(prov::AgentLabel("claude in pane 2".into())),
    };
    let text = round(&list);
    assert!(text.contains(r#""turn":"running""#));
    assert!(text.contains(r#""label":"claude in pane 2""#));
    // A list from an older writer has no turn: it reads as idle.
    let old = r#"{"session":"s-1","rows":[],"keeps":{"last":20,"days":14}}"#;
    let old: CheckpointList = serde_json::from_str(old).expect("old list");
    assert_eq!((old.turn, old.label), (TurnState::Idle, None));
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
        ("agent.checkpoints.turn_max_s", 1800),
    ] {
        assert_eq!(config.value(key), Some(SettingValue::Number(want)), "{key}");
    }
}

#[test]
fn the_checkpoint_requests_and_replies_round_trip_and_name_their_members() {
    let session = SessionId::parse("s-1").expect("session");
    let list = IntentsRequest::CheckpointList {
        session: session.clone(),
    };
    let plan = IntentsRequest::CheckpointPlan {
        session: session.clone(),
        id: CheckpointId(2),
    };
    assert_eq!(list.member(), Member::CheckpointList);
    assert_eq!(plan.member(), Member::CheckpointPlan);
    round(&list);
    round(&plan);
    round(&IntentsReply::Checkpoints(Box::new(CheckpointList {
        session,
        rows: vec![],
        keeps: Retention {
            last: Count(20),
            days: Days(14),
        },
        turn: TurnState::Idle,
        label: None,
    })));
    let ended = IntentsRequest::SessionTurnEnded {
        session: SessionId::parse("s-1").expect("session"),
        turn: TurnId(3),
        how: TurnEnd::Cancelled,
    };
    assert_eq!(ended.member(), Member::SessionTurnEnded);
    round(&ended);
    let watch = IntentsRequest::CheckpointWatch {
        workspace: Workspace::parse("/work/project").expect("workspace"),
        label: "claude in pane 2".into(),
        rewind: Rewind::Agent,
    };
    let mark = IntentsRequest::CheckpointMark {
        session: SessionId::parse("s-2").expect("session"),
    };
    assert_eq!(watch.member(), Member::CheckpointWatch);
    assert_eq!(mark.member(), Member::CheckpointMark);
    round(&watch);
    round(&mark);
    round(&IntentsReply::CheckpointWatching(
        SessionId::parse("s-2").expect("session"),
    ));
    round(&IntentsReply::CheckpointPlan(Ok(RestorePlan::default())));
    round(&IntentsReply::CheckpointPlan(Err(CheckpointFault::Gone)));
}

#[test]
fn a_conversation_moving_between_docket_and_a_terminal_has_its_members_and_shapes() {
    let session = SessionId::parse("s-2").expect("session");
    let agent = WatchedAgent {
        program: ProgramName::parse("claude-code").expect("program"),
        session: AppSessionId::parse("0a1b-2c3d").expect("id"),
    };
    let watch = IntentsRequest::CheckpointWatchAgent {
        workspace: Workspace::parse("/work/project").expect("workspace"),
        label: "claude in pane 2".into(),
        rewind: Rewind::Agent,
        agent: agent.clone(),
    };
    let hand = IntentsRequest::CheckpointHandover {
        session: session.clone(),
        to: HandTo::Terminal,
        agent: Some(agent.clone()),
    };
    assert_eq!(watch.member(), Member::CheckpointWatchAgent);
    assert_eq!(hand.member(), Member::CheckpointHandover);
    assert_eq!(
        IntentsRequest::CheckpointWatched.member(),
        Member::CheckpointWatched
    );
    round(&watch);
    round(&hand);
    round(&IntentsRequest::CheckpointWatched);
    round(&IntentsReply::HandedOver(Some(agent.clone())));
    let handed = Watching {
        session: session.clone(),
        owner: WatchOwner::Companion,
    };
    assert!(round(&IntentsReply::CheckpointWatchingAgent(handed)).contains("companion"));
    round(&IntentsReply::Watched(vec![WatchedLine {
        session,
        agent,
        label: None,
        folder: Some(Workspace::parse("/work/project").expect("workspace")),
    }]));
    // The log's note reads without the agent when an older writer left it out.
    let note: HandNote = serde_json::from_str(r#"{"to":"desktop"}"#).expect("note");
    assert_eq!(
        note,
        HandNote {
            to: HandTo::Desktop,
            agent: None
        }
    );
}

#[test]
fn an_app_conversation_id_is_one_plain_word_and_a_seen_command_line_is_bounded() {
    let long_id = "a".repeat(129);
    let id_cases: &[(&str, bool)] = &[
        ("0a1b-2c3d", true),
        ("a_b.c:d", true),
        ("", false),
        ("-x", false),
        ("a b", false),
        ("a/b", false),
        (long_id.as_str(), false),
    ];
    for (text, ok) in id_cases {
        assert_eq!(AppSessionId::parse(text).is_ok(), *ok, "{text}");
    }
    // The wire form is the plain string, and a bad one does not read.
    assert_eq!(round(&AppSessionId::parse("ab-1").expect("id")), "\"ab-1\"");
    assert!(serde_json::from_str::<AppSessionId>("\"-x\"").is_err());

    let words = |n: usize, each: usize| vec!["w".repeat(each); n];
    let line_cases: &[(Vec<String>, Result<(), SeenCommandFault>)] = &[
        (words(1, 3), Ok(())),
        (words(256, 1), Ok(())),
        (vec![], Err(SeenCommandFault::Empty)),
        (words(257, 1), Err(SeenCommandFault::TooManyWords)),
        (words(2, 40_000), Err(SeenCommandFault::TooLong)),
        (vec!["a\0b".to_owned()], Err(SeenCommandFault::Nul)),
    ];
    for (given, want) in line_cases {
        assert_eq!(SeenCommand::parse(given.clone()).map(drop), *want);
    }
    // Argv can hold secrets: the debug form says only how many words there are.
    let seen =
        SeenCommand::parse(vec!["tool".to_owned(), "--key=hunter2".to_owned()]).expect("seen");
    assert_eq!(format!("{seen:?}"), "SeenCommand(2 words)");
    assert_eq!(seen.tokens().len(), 2);
}
