//! `docket-bringup`, the pure parts: the command line, the refusals, the procs file and its
//! start-time check, the configuration it seeds (no local engine, ever) and the fake proc root.
//! No test starts a daemon or reads anything outside its own temporary directory, except that
//! the stop test points `down` at this test process with a wrong start time to show it is left
//! alone.

use docket_accept::bringup::cli::{Command, UsageError, parse};
use docket_accept::bringup::down::{self, Outcome};
use docket_accept::bringup::procs::{self, Record, State};
use docket_accept::bringup::refuse::{Own, Refusal, check_bus, check_root};
use docket_accept::bringup::{add_command, seed};
use std::path::{Path, PathBuf};

fn words(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_owned).collect()
}

#[test]
fn the_command_line_is_checked() {
    let up = "up --bus unix:path=/tmp/b --root /tmp/r --shell-pid 7";
    let Ok(Command::Up(args)) = parse(&words(&format!("{up} --keys-home /k --catalog /c"))) else {
        panic!("up parses");
    };
    assert_eq!(args.bus, "unix:path=/tmp/b");
    assert_eq!(args.root, PathBuf::from("/tmp/r"));
    assert_eq!(args.shell_pid, 7);
    assert_eq!(args.keys_home, Some(PathBuf::from("/k")));
    assert_eq!(args.catalog, Some(PathBuf::from("/c")));
    assert_eq!(args.inferd_config, None);
    assert!(matches!(
        parse(&words("down --root /tmp/r")),
        Ok(Command::Down(_))
    ));
    assert!(matches!(
        parse(&words("status --root /tmp/r")),
        Ok(Command::Status(_))
    ));

    let bad: [(&str, UsageError); 9] = [
        ("", UsageError::Subcommand),
        ("restart --root /r", UsageError::Subcommand),
        ("up --root /r --shell-pid 7", UsageError::Missing("--bus")),
        (
            "up --bus unix:path=/b --root /r",
            UsageError::Missing("--shell-pid"),
        ),
        (
            "up --bus unix:path=/b --root /r --shell-pid 0",
            UsageError::Pid("0".to_owned()),
        ),
        (
            "up --bus unix:path=/b --root r --shell-pid 7",
            UsageError::Relative("--root", PathBuf::from("r")),
        ),
        (
            "up --bus unix:path=/b --root /r --shell-pid 7 --keys-home k",
            UsageError::Relative("--keys-home", PathBuf::from("k")),
        ),
        (
            "down --root /r --bus unix:path=/b",
            UsageError::Unknown("--bus".to_owned()),
        ),
        ("status --root", UsageError::NoValue("--root".to_owned())),
    ];
    for (line, fault) in bad {
        assert_eq!(parse(&words(line)), Err(fault), "{line}");
    }
}

fn own() -> Own {
    Own {
        home: Some(PathBuf::from("/home/someone")),
        session_bus: Some("unix:path=/run/user/1000/bus".to_owned()),
        system_bus: None,
    }
}

#[test]
fn a_root_in_a_checkout_or_the_home_is_refused() {
    let tmp = tempfile::tempdir().expect("tmp");
    let clean = tmp.path().join("scratch");
    assert_eq!(check_root(&clean, &own()), Ok(()));

    std::fs::create_dir(tmp.path().join(".git")).expect("git dir");
    let inside = tmp.path().join("deep/scratch");
    assert_eq!(
        check_root(&inside, &own()),
        Err(Refusal::RootInGit(inside.clone(), tmp.path().to_owned()))
    );

    let refused: [(&str, Refusal); 4] = [
        (
            "/home/someone",
            Refusal::RootTooBroad("/home/someone".into()),
        ),
        ("/", Refusal::RootTooBroad("/".into())),
        ("scratch", Refusal::RootNotAbsolute("scratch".into())),
        ("/tmp/a/../b", Refusal::RootDotDot("/tmp/a/../b".into())),
    ];
    for (root, fault) in refused {
        assert_eq!(check_root(Path::new(root), &own()), Err(fault), "{root}");
    }
}

#[test]
fn the_real_buses_are_refused() {
    let private = "unix:path=/tmp/demo/bus.sock,guid=abc";
    assert_eq!(check_bus(private, &own()), Ok(()));
    assert_eq!(check_bus(private, &Own::default()), Ok(()));

    let refused = [
        ("unix:path=/run/user/1000/bus", "own"),
        ("unix:path=/run/user/1000/bus,guid=ff", "own"),
        ("unix:path=/run/user/1001/bus", "real"),
        ("unix:path=/run/dbus/system_bus_socket", "real"),
        ("unix:path=/var/run/dbus/system_bus_socket", "real"),
        ("tcp:host=localhost,port=1", "not unix"),
        ("", "not unix"),
    ];
    for (bus, why) in refused {
        let got = check_bus(bus, &own());
        let right = match why {
            "own" => matches!(got, Err(Refusal::BusIsOwn(_))),
            "real" => matches!(got, Err(Refusal::BusLooksReal(_))),
            _ => matches!(got, Err(Refusal::BusNotUnix(_))),
        };
        assert!(right, "{bus}: {got:?}");
    }
}

fn stat_line(comm: &str, state: char, start: u64) -> String {
    format!("42 ({comm}) {state} {}{start} 0 0\n", "0 ".repeat(18))
}

#[test]
fn the_procs_file_round_trips_and_the_start_time_decides() {
    let records = vec![
        Record {
            name: "inferd".into(),
            pid: 10,
            start: 111,
        },
        Record {
            name: "companiond".into(),
            pid: 11,
            start: 222,
        },
    ];
    assert_eq!(procs::parse(&procs::render(&records)), Ok(records.clone()));
    assert_eq!(
        procs::render(&records),
        "inferd 10 111\ncompaniond 11 222\n"
    );
    for line in ["inferd 10", "inferd x 1", "inferd 10 1 extra"] {
        assert!(procs::parse(line).is_err(), "{line}");
    }

    let tmp = tempfile::tempdir().expect("tmp");
    assert_eq!(procs::read(tmp.path()), Ok(Vec::new()));
    procs::write(tmp.path(), &records).expect("written");
    assert_eq!(procs::read(tmp.path()), Ok(records.clone()));

    // The command name may hold spaces and parentheses; fields count from the last `)`.
    assert_eq!(
        procs::parse_stat(&stat_line("a b) (c", 'S', 111)),
        Some(('S', 111))
    );
    assert_eq!(procs::parse_stat("garbage"), None);
    let first = &records[0];
    let table = [
        (Some(stat_line("inferd", 'S', 111)), State::Running),
        (Some(stat_line("inferd", 'R', 111)), State::Running),
        (Some(stat_line("other", 'S', 999)), State::Gone),
        (Some(stat_line("inferd", 'Z', 111)), State::Gone),
        (None, State::Gone),
    ];
    for (stat, want) in table {
        assert_eq!(first.state_in(stat.as_deref()), want, "{stat:?}");
    }
}

#[test]
fn down_leaves_a_pid_with_another_start_time_alone() {
    let tmp = tempfile::tempdir().expect("tmp");
    // This very process, recorded with a start time it does not have: the pid was reused.
    let stranger = Record {
        name: "inferd".into(),
        pid: std::process::id(),
        start: 1,
    };
    procs::write(tmp.path(), std::slice::from_ref(&stranger)).expect("written");
    let done = down::down(tmp.path()).expect("down");
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].outcome, Outcome::AlreadyGone);
    let listed = down::status(tmp.path()).expect("status");
    assert_eq!(listed, vec![(stranger, State::Gone)]);
    assert!(procs::file(tmp.path()).exists(), "the root stays on disk");
}

fn parsed(text: &str) -> toml::Table {
    text.parse().expect("valid TOML")
}

#[test]
fn inferd_is_never_given_a_local_engine() {
    let root = Path::new("/tmp/demo-root");
    let table = parsed(&seed::inferd_toml(root, "").expect("seeded"));
    let engines = table["engines"].as_table().expect("an [engines] table");
    assert!(
        engines.is_empty(),
        "no engine program, no attached engine: {engines:?}"
    );
    let probe = table["probe"].as_table().expect("a [probe] table");
    for runtime in ["ollama", "llama_cpp", "lm_studio"] {
        let ports = probe[runtime].as_array().expect("a list of ports");
        assert!(ports.is_empty(), "{runtime} is probed");
    }
    assert!(table["callers"]["apps"]["org.quire.Companion"].is_array());

    // The person's own rows are kept, but cannot bring an engine, a probe or other callers back.
    let extra = "[ai]\nlocal_only = \"off\"\n";
    let with = parsed(&seed::inferd_toml(root, extra).expect("extra rows"));
    assert_eq!(with["ai"]["local_only"].as_str(), Some("off"));
    assert!(with["engines"].as_table().is_some_and(|t| t.is_empty()));
    let refused = [
        (
            "[engines.attached.\"m\"]\nurl = \"http://127.0.0.1:1\"\n",
            "engines",
        ),
        ("[engines]\nvllm_python = \"/p\"\n", "engines"),
        ("[probe]\nollama = [11434]\n", "probe"),
        ("[callers]\nshell = [\"x.scope\"]\n", "callers"),
    ];
    for (rows, table) in refused {
        assert_eq!(
            seed::inferd_toml(root, rows),
            Err(seed::ExtraFault::Owned(table)),
            "{rows}"
        );
    }
    assert!(matches!(
        seed::inferd_toml(root, "= nonsense"),
        Err(seed::ExtraFault::Toml(_))
    ));
}

#[test]
fn the_shell_is_sill_in_the_fake_proc_root() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = tmp.path();
    seed::layout(root).expect("layout");
    for sub in ["logs", "data", "config", "cache", "run", "keys", "proc"] {
        assert!(root.join(sub).is_dir(), "{sub}");
    }
    let text = seed::inferd_toml(root, "").expect("seeded");
    seed::files(root, 4242, &text).expect("files");

    let cgroup = std::fs::read_to_string(root.join("proc/4242/cgroup")).expect("cgroup row");
    assert!(cgroup.starts_with("0::/user.slice/"), "{cgroup}");
    assert!(cgroup.ends_with("/sill.service\n"), "{cgroup}");
    let main = std::fs::read_to_string(root.join("proc/units/sill.service")).expect("unit row");
    assert_eq!(main, "4242");

    // memoryd gives that unit the shell's role.
    let callers = parsed(
        &std::fs::read_to_string(root.join("config/quire/memory-callers.toml")).expect("callers"),
    );
    let shell = callers["caller"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|row| row["unit"].as_str() == Some("sill.service"))
        .expect("a sill.service row");
    assert_eq!(shell["app"].as_str(), Some("org.quire.Shell"));
    assert_eq!(shell["role"].as_str(), Some("sheet_host"));
    assert!(root.join("data/quire/memory/spaces.toml").is_file());
    assert_eq!(
        std::fs::read_to_string(root.join("config/quire/inferd.toml")).expect("inferd.toml"),
        text
    );
}

#[test]
fn the_key_command_targets_the_root() {
    let line = add_command(Path::new("/tmp/demo root"), Path::new("/opt/inferd"));
    assert!(line.starts_with("env -i "), "{line}");
    assert!(line.contains("'HOME=/tmp/demo root'"), "{line}");
    assert!(line.contains("INFERD_KEYS=file:/tmp/demo"), "{line}");
    assert!(
        line.ends_with(
            "/opt/inferd add openrouter --allow org.quire.Companion --allow org.quire.Intents"
        ),
        "{line}"
    );
}
