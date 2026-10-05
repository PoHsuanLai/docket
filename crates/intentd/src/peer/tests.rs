use super::*;
use docket_core::CallerRole;

fn config() -> IntentdConfig {
    IntentdConfig::shipped().expect("the shipped configuration")
}

fn names(texts: &[&str]) -> Vec<AppName> {
    texts
        .iter()
        .map(|t| AppName::parse(t).expect("name"))
        .collect()
}

fn who(owned: &[&str], process: Process) -> Option<(String, Vec<CallerRole>)> {
    let facts = Facts {
        names: names(owned),
        process,
    };
    derive(&config(), &facts).map(|c| (c.app.name.to_string(), c.roles.into_iter().collect()))
}

fn app(text: &str) -> Process {
    Process::App(AppName::parse(text).expect("app"))
}

#[test]
fn a_table_of_connections_and_who_they_are() {
    /// One connection: what it owns, what its cgroup says, and who it is.
    struct Row {
        what: &'static str,
        owned: &'static [&'static str],
        process: Process,
        expected: Option<(&'static str, &'static [CallerRole])>,
    }
    let row = |what, owned, process, expected| Row {
        what,
        owned,
        process,
        expected,
    };
    const SILL: &[CallerRole] = &[
        CallerRole::Launcher,
        CallerRole::Confirm,
        CallerRole::Control,
    ];
    let rows = [
        row(
            "a terminal's child owns no name and is the cli role",
            &[],
            Process::Terminal,
            Some(("org.quire.Do", &[CallerRole::Cli])),
        ),
        row(
            "an unknown process with no name is nobody",
            &[],
            Process::Unknown,
            None,
        ),
        row(
            "an app scope with no name is that app, a plain one",
            &[],
            app("org.example.Thing"),
            Some(("org.example.Thing", &[])),
        ),
        row(
            "an app scope the configuration lists plays its roles",
            &[],
            app("org.quire.Do"),
            Some(("org.quire.Do", &[CallerRole::Cli])),
        ),
        row(
            "sill owns several names and plays the roles of the one that is listed",
            &["org.quire.Confirm1", "org.quire.Shell"],
            Process::Unknown,
            Some(("org.quire.Shell", SILL)),
        ),
        row(
            "an app that owns only its own name is a plain app",
            &["org.quire.Mail"],
            Process::Unknown,
            Some(("org.quire.Mail", &[])),
        ),
        row(
            "the name wins over the cgroup: a terminal child that owns a name is that name",
            &["org.quire.Mail"],
            Process::Terminal,
            Some(("org.quire.Mail", &[])),
        ),
        row(
            "companiond",
            &["org.quire.Companion1"],
            Process::Unknown,
            Some(("org.quire.Companion1", &[CallerRole::Companion])),
        ),
        row(
            "a name that is listed beats one that is not, whatever the order",
            &["org.aaa.First", "org.quire.Reader1"],
            Process::Unknown,
            Some(("org.quire.Reader1", &[CallerRole::Reader])),
        ),
        row(
            "an unlisted name alone is a plain app under that name",
            &["org.example.Thing"],
            Process::Unknown,
            Some(("org.example.Thing", &[])),
        ),
    ];
    for r in rows {
        let expected = r.expected.map(|(n, roles)| (n.to_owned(), roles.to_vec()));
        assert_eq!(who(r.owned, r.process), expected, "{}", r.what);
    }
}

#[test]
fn the_identity_is_unsandboxed_until_something_better_is_known() {
    let facts = Facts {
        names: names(&["org.quire.Mail"]),
        process: Process::Unknown,
    };
    let caller = derive(&config(), &facts).expect("caller");
    assert_eq!(caller.app.isolation, Isolation::Unsandboxed);
}

/// A fake proc root with process `pid` in the cgroup `leaf` under the user's slice.
fn proc_with(pid: u32, leaf: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("scratch");
    let at = dir.path().join(pid.to_string());
    std::fs::create_dir_all(&at).expect("pid dir");
    let slice = "0::/user.slice/user-1000.slice/user@1000.service/app.slice";
    std::fs::write(at.join("cgroup"), format!("{slice}/{leaf}\n")).expect("cgroup");
    dir
}

#[test]
fn a_cgroup_names_the_process_and_only_a_terminal_scope_is_the_cli() {
    let rows: [(&str, Process); 11] = [
        ("vte-spawn-1b2c.scope", Process::Terminal),
        ("tmux-spawn-9.scope", Process::Terminal),
        ("session-3.scope", Process::Terminal),
        ("app-org.kde.konsole-123.scope", app("org.kde.konsole")),
        (
            "app-gnome-org.gnome.Terminal-77.scope",
            app("org.gnome.Terminal"),
        ),
        (
            "app-flatpak-org.example.Thing-5.scope",
            app("org.example.Thing"),
        ),
        ("intentd.service", Process::Unknown),
        ("dbus-broker.service", Process::Unknown),
        ("vte-spawn-1b2c.service", Process::Unknown),
        ("session-3.slice", Process::Unknown),
        ("run-r4242.scope", Process::Unknown),
    ];
    for (leaf, expected) in rows {
        let root = proc_with(41, leaf);
        assert_eq!(process_of(root.path(), 41), expected, "{leaf}");
    }
}

#[test]
fn a_process_with_no_readable_cgroup_is_nobody() {
    let root = tempfile::tempdir().expect("scratch");
    assert_eq!(process_of(root.path(), 7), Process::Unknown, "no file");
    let at = root.path().join("8");
    std::fs::create_dir_all(&at).expect("dir");
    std::fs::write(at.join("cgroup"), "garbage\n").expect("file");
    assert_eq!(
        process_of(root.path(), 8),
        Process::Unknown,
        "not a cgroup line"
    );
}

#[test]
fn a_terminal_child_of_the_fixture_is_the_cli_role_end_to_end() {
    let root = proc_with(55, "vte-spawn-aa.scope");
    let found = process_of(root.path(), 55);
    let caller = derive(
        &config(),
        &Facts {
            names: Vec::new(),
            process: found,
        },
    )
    .expect("a caller");
    assert_eq!(caller.app.name.as_str(), "org.quire.Do");
    assert_eq!(
        caller.roles.into_iter().collect::<Vec<_>>(),
        [CallerRole::Cli]
    );
}
