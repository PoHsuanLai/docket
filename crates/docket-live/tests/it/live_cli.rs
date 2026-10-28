//! The command line of the live harness: the table of what it accepts and refuses, the model-source
//! flag, and the key home.

use crate::support::words;
use docket_accept::world::ModelSource;
use docket_live::cli::{Command, PlannerMode, UsageError, parse};
use docket_live::engine::{Engine, EngineError};

#[test]
fn the_command_line_table() {
    let Ok(Command::Corpus(c)) = parse(&words(
        "corpus --engine cloud --inferd-config c.toml --label v1 --corpus injection --corpus benign --timeout-ms 5000 --fnr-max-permille 50",
    )) else {
        panic!("corpus")
    };
    assert_eq!(c.engine, Engine::Cloud);
    assert_eq!(c.corpora, ["injection", "benign"]);
    assert_eq!(c.timeout_ms, Some(5000));
    assert_eq!(c.fnr_max_permille, Some(50));
    assert_eq!(c.label, "v1");
    assert_eq!(c.planner, PlannerMode::Scripted, "scripted unless asked");
    let Ok(Command::Corpus(p)) = parse(&words(
        "corpus --engine local --inferd-config c.toml --corpus attacks --planner live",
    )) else {
        panic!("live planner")
    };
    assert_eq!(
        (p.planner, p.corpora),
        (PlannerMode::Live, vec!["attacks".to_owned()])
    );
    let Ok(Command::Corpus(q)) = parse(&words("corpus --engine scripted --planner scripted"))
    else {
        panic!("scripted planner")
    };
    assert_eq!(q.planner, PlannerMode::Scripted);
    let Ok(Command::Smoke(s)) = parse(&words("smoke --engine scripted --flow flow-c")) else {
        panic!("smoke")
    };
    assert_eq!(
        (s.engine, s.flows, s.patience_s),
        (Engine::Scripted, vec!["flow-c".to_owned()], 600)
    );
    let cases = [
        ("", UsageError::Subcommand),
        (
            "corpus",
            UsageError::Engine("--engine is required".to_owned()),
        ),
        (
            "corpus --engine scripted --bogus",
            UsageError::Unknown("--bogus".to_owned()),
        ),
        (
            "corpus --engine scripted --label",
            UsageError::NoValue("--label".to_owned()),
        ),
        (
            "smoke --engine scripted --patience-s soon",
            UsageError::Number("--patience-s".to_owned(), "soon".to_owned()),
        ),
        ("run --engine scripted", UsageError::Subcommand),
        (
            "corpus --engine local --planner clever",
            UsageError::Planner("--planner takes scripted or live, not \"clever\"".to_owned()),
        ),
        (
            "corpus --engine local --planner",
            UsageError::NoValue("--planner".to_owned()),
        ),
        (
            "corpus --engine scripted --planner live",
            UsageError::Planner(
                "--planner live needs a live engine (--engine local or cloud): a cassette has no planner to play"
                    .to_owned(),
            ),
        ),
        (
            "smoke --engine local --planner live",
            UsageError::Unknown("--planner (corpus only)".to_owned()),
        ),
    ];
    for (line, want) in cases {
        assert_eq!(parse(&words(line)).err(), Some(want), "{line}");
    }
}

#[test]
fn a_live_engine_needs_the_owners_config_and_never_a_callers_table() {
    assert_eq!(Engine::parse("local").map(Engine::slug).ok(), Some("local"));
    assert!(matches!(
        Engine::parse("fast"),
        Err(EngineError::Unknown(_))
    ));
    let none = Engine::Local.source(String::new(), None);
    assert!(matches!(none, Err(EngineError::NoConfig("local"))));
    let dir = tempfile::tempdir().expect("scratch");
    let with = dir.path().join("with.toml");
    std::fs::write(&with, "[callers]\ncua = [\"x.service\"]\n").expect("write");
    assert!(matches!(
        Engine::Cloud.source(String::new(), Some(&with)),
        Err(EngineError::HasCallers)
    ));
    let fine = dir.path().join("fine.toml");
    std::fs::write(&fine, "[ai]\nlocal_only = \"off\"\n").expect("write");
    assert!(matches!(
        Engine::Cloud.source(String::new(), Some(&fine)),
        Ok(ModelSource::Live(_))
    ));
    assert!(matches!(
        Engine::Scripted.source("x".to_owned(), None),
        Ok(ModelSource::Scripted(text)) if text == "x"
    ));
}

mod keys_home {
    use super::words;
    use docket_accept::keys_home::{HomeFault, KeysHome, check_dir};
    use docket_live::cli::{Command, UsageError, parse};
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn mode(path: &Path, bits: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits)).expect("chmod");
    }

    #[test]
    fn the_option_needs_an_absolute_path_and_the_old_flags_say_where_they_went() {
        let Ok(Command::Corpus(c)) = parse(&words("corpus --engine cloud --keys-home /h")) else {
            panic!("parses");
        };
        assert_eq!(c.keys_home.as_deref(), Some(Path::new("/h")));
        let no_flag = parse(&words("corpus --engine scripted"));
        assert!(matches!(no_flag, Ok(Command::Corpus(c)) if c.keys_home.is_none()));
        for line in [
            "corpus --engine cloud --keys-home rel",
            "corpus --engine cloud --accountd /a --keys-home /h",
            "corpus --engine cloud --accountd-home /h",
        ] {
            assert!(
                matches!(parse(&words(line)), Err(UsageError::Home(_))),
                "{line}"
            );
        }
        let moved = parse(&words("corpus --engine cloud --accountd-home /h"))
            .expect_err("moved")
            .to_string();
        assert!(moved.contains("--keys-home"), "{moved}");
    }

    #[test]
    fn a_loose_directory_or_one_in_a_git_work_tree_is_refused() {
        let tmp = tempfile::tempdir().expect("scratch");
        let loose = tmp.path().join("loose");
        std::fs::create_dir(&loose).expect("dir");
        mode(&loose, 0o750);
        assert!(matches!(check_dir(&loose), Err(HomeFault::LooseMode(..))));
        mode(&loose, 0o700);
        assert!(check_dir(&loose).is_ok());
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).expect("git");
        let inside = repo.join("keys");
        std::fs::create_dir(&inside).expect("dir");
        mode(&inside, 0o700);
        assert!(matches!(check_dir(&inside), Err(HomeFault::InsideGit(..))));
        assert!(matches!(
            check_dir(Path::new("relative")),
            Err(HomeFault::NotAbsolute(_))
        ));
    }

    #[test]
    fn an_empty_home_says_how_to_fill_it_and_a_filled_one_is_pointed_at_not_copied() {
        let tmp = tempfile::tempdir().expect("scratch");
        let home = tmp.path().join("home");
        std::fs::create_dir(&home).expect("dir");
        mode(&home, 0o700);
        let why = KeysHome::open(&home).expect_err("empty").to_string();
        assert!(why.contains("dev/live/cloud-key.sh"), "{why}");
        std::fs::write(home.join("accountd.keys"), "fake").expect("keys");
        mode(&home.join("accountd.keys"), 0o644);
        assert!(matches!(
            KeysHome::open(&home),
            Err(HomeFault::LooseMode(..))
        ));
        mode(&home.join("accountd.keys"), 0o600);
        let registry = home.join("state/porter");
        std::fs::create_dir_all(&registry).expect("state");
        std::fs::write(registry.join("registry.json"), "{}").expect("registry");
        let open = KeysHome::open(&home).expect("filled");
        assert!(open.keys_setting().ends_with("/accountd.keys"));
        let world = tmp.path().join("world");
        open.seed(&world).expect("seed");
        assert!(world.join(".local/state/porter/registry.json").is_file());
        assert!(!world.join("accountd.keys").exists());
    }

    #[test]
    fn smoke_takes_the_cloud_options_and_a_repeat() {
        let Ok(Command::Smoke(s)) = parse(&words("smoke --engine cloud --keys-home /h --repeat 5"))
        else {
            panic!("parses");
        };
        assert_eq!(
            (s.repeat, s.keys_home.as_deref()),
            (5, Some(Path::new("/h")))
        );
        let Ok(Command::Smoke(plain)) = parse(&words("smoke --engine scripted")) else {
            panic!("parses");
        };
        assert_eq!(plain.repeat, 1);
        for line in [
            "smoke --engine cloud --accountd /a",
            "smoke --engine cloud --keys-home rel",
            "smoke --engine cloud --repeat 0",
            "corpus --engine cloud --repeat 2",
        ] {
            assert!(parse(&words(line)).is_err(), "{line}");
        }
    }
}
