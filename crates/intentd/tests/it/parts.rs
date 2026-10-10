//! intentd's parts without a bus: the configuration, the built-in manifests, the record kinds,
//! the queue, the memory seam and the clock.

use almanac_client::Absent;
use docket_core::*;
use docket_router::{EventSink, LinkFault, MemoryLink};
use intentd::*;
use porter_core::AppName;
use prov::{ConfirmId, Effect, SpaceScope, UnixSeconds};
use std::path::PathBuf;

fn fixture() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/intentd.toml");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn the_fixture_parses_and_one_name_may_play_several_roles() {
    let config = IntentdConfig::parse(&fixture()).expect("a configuration");
    let sill = AppName::parse("org.quire.Sill").expect("app");
    let roles = config.roles_of(&sill);
    assert_eq!(
        roles.into_iter().collect::<Vec<_>>(),
        [
            CallerRole::Launcher,
            CallerRole::Confirm,
            CallerRole::Control
        ]
    );
    let quire_do = AppName::parse("org.quire.Do").expect("app");
    assert_eq!(
        config.roles_of(&quire_do).into_iter().collect::<Vec<_>>(),
        [CallerRole::Cli],
        "any process running quire-do plays the cli role"
    );
    let temor = AppName::parse("org.quire.Temor").expect("app");
    assert_eq!(
        config.roles_of(&temor).into_iter().collect::<Vec<_>>(),
        [CallerRole::Cli]
    );
    let plain = AppName::parse("org.quire.Mail").expect("app");
    assert!(
        config.roles_of(&plain).is_empty(),
        "an unlisted name is a plain app"
    );
    assert_eq!(config.agent, AgentConfig::default());
}

#[test]
fn a_second_opinion_of_the_same_family_does_not_load() {
    let same = fixture().replace("family = \"other\"", "family = \"holo\"");
    assert_eq!(IntentdConfig::parse(&same), Err(ConfigError::SameFamily));
    assert!(matches!(
        IntentdConfig::parse("roles = 3"),
        Err(ConfigError::Toml(_))
    ));
}

#[test]
fn the_built_in_manifests_validate_and_carry_their_prefixes() {
    let manifests = builtin_manifests().expect("shipped manifests are valid");
    let apps: Vec<String> = manifests
        .iter()
        .map(|m| m.manifest().app.to_string())
        .collect();
    assert_eq!(
        apps,
        [
            "org.quire.Memory",
            "org.quire.Companion",
            "org.quire.AcpAgent",
            "org.quire.Checkpoints"
        ]
    );
    let companion = &manifests[1];
    assert!(
        companion
            .manifest()
            .actions
            .iter()
            .any(|a| a.name.as_str() == "companion.task.start")
    );
}

#[test]
fn every_record_has_a_header_kind_in_the_dotted_grammar() {
    let at = UnixSeconds(1);
    let space = prov::SpaceId::parse("work").expect("space");
    let halt = AuditRecord::Halt {
        at,
        scope: SpaceScope::Only(space),
        cause: HaltCause::StopKey,
    };
    let confirm = AuditRecord::Confirm {
        at,
        id: ConfirmId::parse("c-1").expect("id"),
        answer: ConfirmAnswerKind::Ended(ConfirmEnd::Expired),
        input: None,
    };
    let breaker = AuditRecord::Breaker {
        at,
        session: prov::SessionId::parse("s-1").expect("s"),
        trip: BreakerTrip::Probing,
    };
    let cases = [
        (halt, "docket.halt"),
        (confirm, "docket.confirm"),
        (breaker, "docket.breaker"),
    ];
    for (record, want) in cases {
        assert_eq!(
            kind_tag_of(&record)
                .map(|k| k.as_str().to_owned())
                .as_deref(),
            Some(want)
        );
    }
    let _ = Effect::Read;
}

#[test]
fn the_queue_hands_over_everything_oldest_first_once() {
    let sink = QueuedSink::new();
    let at = |t| UnixSeconds(t);
    sink.append(AuditRecord::Halt {
        at: at(1),
        scope: SpaceScope::Any,
        cause: HaltCause::KillChord,
    });
    sink.append(AuditRecord::Halt {
        at: at(2),
        scope: SpaceScope::Any,
        cause: HaltCause::ControlCentre,
    });
    let drained = sink.drain();
    assert_eq!(drained.len(), 2);
    assert!(matches!(
        drained[0],
        AuditRecord::Halt {
            at: UnixSeconds(1),
            ..
        }
    ));
    assert!(sink.drain().is_empty(), "drained records are gone");
}

#[tokio::test]
async fn no_memory_on_the_desktop_is_an_unavailable_link_not_a_panic() {
    let memory = AlmanacMemory::over(Absent);
    let space = prov::SpaceId::parse("work").expect("space");
    let got = memory.ask(almanac_core::MemoryRequest::Spaces).await;
    assert_eq!(got, Err(LinkFault::Unavailable));
    let _ = space;
}

#[test]
fn the_binary_with_no_bus_to_serve_on_says_so_and_exits_one() {
    let exe = env!("CARGO_BIN_EXE_intentd");
    let dir = tempfile::tempdir().expect("scratch");
    let nowhere = format!("unix:path={}", dir.path().join("none.sock").display());
    let output = std::process::Command::new(exe)
        .env_clear()
        .env("HOME", dir.path())
        .env("XDG_DATA_HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("DBUS_SESSION_BUS_ADDRESS", nowhere)
        .output()
        .expect("runs");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("intentd: "));
}

#[test]
fn every_unit_carries_the_sandbox_lines_and_every_activation_file_names_a_unit() {
    let dist = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist");
    let mut units = Vec::new();
    for entry in std::fs::read_dir(&dist).expect("dist") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "service") {
            units.push(path);
        }
    }
    assert!(
        units.len() >= 5,
        "intentd, companiond, readerd, voiced and actions-mcp"
    );
    for path in &units {
        let text = std::fs::read_to_string(path).expect("unit");
        for line in [
            "RestrictAddressFamilies=AF_UNIX",
            "PrivateNetwork=yes",
            "ProtectSystem=strict",
            "NoNewPrivileges=yes",
            "MemoryDenyWriteExecute=yes",
            "SystemCallFilter=@system-service",
            "CapabilityBoundingSet=",
        ] {
            assert!(
                text.lines().any(|l| l == line),
                "{} lacks {line}",
                path.display()
            );
        }
    }
    for entry in std::fs::read_dir(dist.join("dbus")).expect("dbus") {
        let path = entry.expect("entry").path();
        let text = std::fs::read_to_string(&path).expect("activation file");
        let unit = text
            .lines()
            .find_map(|l| l.strip_prefix("SystemdService="))
            .expect("SystemdService");
        assert!(
            dist.join(unit).exists(),
            "{} names {unit}, which is not in dist/",
            path.display()
        );
    }
}

#[test]
fn intentds_unit_ends_with_the_graphical_session_and_makes_the_directory_it_may_write() {
    let dist = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist");
    let text = std::fs::read_to_string(dist.join("intentd.service")).expect("unit");
    let has = |line: &str| text.lines().any(|l| l == line);
    assert!(
        has("PartOf=graphical-session.target"),
        "grants end with the login"
    );
    assert!(has("Type=dbus") && has("BusName=org.quire.Intents1"));
    // A read-write path that does not exist stops a sandboxed unit, so it is made first, outside
    // the sandbox.
    assert!(has(
        "ExecStartPre=+/usr/bin/mkdir -p %h/.local/share/quire/intents"
    ));
    assert!(has("ReadWritePaths=%h/.local/share/quire/intents"));
    let activation = std::fs::read_to_string(dist.join("dbus/org.quire.Intents1.service"))
        .expect("activation file");
    assert!(activation.contains("Name=org.quire.Intents1"));
    assert!(activation.contains("SystemdService=intentd.service"));
}
