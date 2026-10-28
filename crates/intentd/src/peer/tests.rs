use super::*;
use crate::callers;
use docket_dbus::callers::CallerTable;
use porter_callers::ProcCallers;

fn config() -> IntentdConfig {
    IntentdConfig::shipped().expect("the shipped configuration")
}

fn held(app: &str, role: Role) -> Found {
    Found::Process(Caller {
        app: AppId {
            name: AppName::parse(app).expect("app"),
            isolation: Isolation::Unsandboxed,
        },
        role,
    })
}

fn who(found: &Found, acp: &AcpGate) -> Option<(String, Vec<CallerRole>)> {
    derive(&config(), acp, found).map(|c| (c.app.name.to_string(), c.roles.into_iter().collect()))
}

const SILL: &[CallerRole] = &[
    CallerRole::Launcher,
    CallerRole::Confirm,
    CallerRole::Control,
];

#[test]
fn a_table_of_callers_and_the_roles_their_standing_backs() {
    /// One caller: what the table or the cgroup made of it, and who it is.
    struct Row {
        what: &'static str,
        found: Found,
        expected: Option<(&'static str, &'static [CallerRole])>,
    }
    let row = |what, found, expected| Row {
        what,
        found,
        expected,
    };
    let rows = [
        row(
            "a terminal's child is the cli role",
            Found::Terminal,
            Some(("org.quire.Do", &[CallerRole::Cli])),
        ),
        row(
            "an app scope the configuration does not list is a plain app",
            held("org.example.Thing", Role::App),
            Some(("org.example.Thing", &[])),
        ),
        row(
            "an app scope the configuration lists plays its roles",
            held("org.quire.Do", Role::App),
            Some(("org.quire.Do", &[CallerRole::Cli])),
        ),
        row(
            "the shell's unit, with the system's sheet_host role, plays the sheet roles",
            held("org.quire.Shell", Role::SheetHost),
            Some(("org.quire.Shell", SILL)),
        ),
        row(
            "a scope that claims the shell's app is a plain app and plays no sheet role",
            held("org.quire.Shell", Role::App),
            Some(("org.quire.Shell", &[])),
        ),
        row(
            "the shell's app with the agent role plays no sheet role either",
            held("org.quire.Shell", Role::Agent),
            Some(("org.quire.Shell", &[])),
        ),
        row(
            "companiond's unit plays the companion role",
            held("org.quire.Companion1", Role::Agent),
            Some(("org.quire.Companion1", &[CallerRole::Companion])),
        ),
        row(
            "an app scope called like companiond plays nothing",
            held("org.quire.Companion1", Role::App),
            Some(("org.quire.Companion1", &[])),
        ),
        row(
            "readerd's unit plays the reader role",
            held("org.quire.Reader1", Role::Agent),
            Some(("org.quire.Reader1", &[CallerRole::Reader])),
        ),
        row(
            "cuad's unit plays the cua role, an agent of that app does not",
            held("org.quire.Cua1", Role::Cua),
            Some(("org.quire.Cua1", &[CallerRole::Cua])),
        ),
        row(
            "an agent in cuad's app is not cuad",
            held("org.quire.Cua1", Role::Agent),
            Some(("org.quire.Cua1", &[])),
        ),
    ];
    for r in rows {
        let expected = r.expected.map(|(n, roles)| (n.to_owned(), roles.to_vec()));
        assert_eq!(who(&r.found, &AcpGate::shut()), expected, "{}", r.what);
    }
}

#[test]
fn the_acp_apps_count_only_while_their_settings_are_on() {
    let acp = held("org.quire.Acp", Role::Agent);
    assert_eq!(who(&acp, &AcpGate::shut()), None);
    let on = AcpGate::new(docket_settings::AcpExpose::On);
    assert_eq!(
        who(&acp, &on),
        Some((
            "org.quire.Acp".to_owned(),
            vec![CallerRole::Editor, CallerRole::Companion]
        ))
    );
    let host = held("org.quire.AcpAgent", Role::Agent);
    assert_eq!(who(&host, &on), None);
    on.set_agents(docket_settings::AcpAgents::On);
    assert_eq!(
        who(&host, &on),
        Some(("org.quire.AcpAgent".to_owned(), vec![CallerRole::AcpAgent]))
    );
}

#[test]
fn the_identity_is_unsandboxed_until_something_better_is_known() {
    let caller = derive(
        &config(),
        &AcpGate::shut(),
        &held("org.quire.Mail", Role::App),
    )
    .expect("caller");
    assert_eq!(caller.app.isolation, Isolation::Unsandboxed);
}

/// Process `pid` in the fake proc root, in the cgroup `leaf` under the user's slice; a service's
/// process is also its unit's main process when `main` says so.
fn put(root: &Path, pid: u32, leaf: &str, main: Main) {
    let at = root.join(pid.to_string());
    std::fs::create_dir_all(&at).expect("pid dir");
    let slice = "0::/user.slice/user-1000.slice/user@1000.service/app.slice";
    std::fs::write(at.join("cgroup"), format!("{slice}/{leaf}\n")).expect("cgroup");
    if main == Main::Yes {
        let units = root.join("units");
        std::fs::create_dir_all(&units).expect("units dir");
        std::fs::write(units.join(leaf), pid.to_string()).expect("main pid");
    }
}

/// Whether a process is the main process of the unit it runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Main {
    Yes,
    No,
}

/// What the table and the fake proc root make of process `pid`.
fn found_in(root: &Path, pid: u32, table: &CallerTable) -> Option<Found> {
    classify(
        root,
        Some(pid),
        ProcCallers::caller_of_pid(root, pid, table),
    )
}

#[test]
fn a_process_is_identified_by_its_unit_and_a_scope_or_a_stranger_is_not_the_unit() {
    let root = tempfile::tempdir().expect("scratch");
    let root = root.path();
    let table = callers::shipped();
    for (pid, leaf, main) in [
        (1, "sill.service", Main::Yes),
        (2, "sill-shell.scope", Main::No),
        (3, "sill.service", Main::No),
        (4, "companiond.service", Main::Yes),
        (5, "app-org.quire.Companion1-1.scope", Main::No),
        (6, "vte-spawn-1b2c.scope", Main::No),
        (7, "curl.service", Main::Yes),
        (8, "docket-acp.scope", Main::No),
        (9, "cuad.service", Main::Yes),
        (10, "app-org.quire.Cua1-1.scope", Main::No),
        (11, "run-r4242.scope", Main::No),
    ] {
        put(root, pid, leaf, main);
    }
    // A scope has no main process: the editor's host is matched by the program it runs.
    std::os::unix::fs::symlink("/usr/bin/docket-acp", root.join("8").join("exe")).expect("exe");
    let on = AcpGate::new(docket_settings::AcpExpose::On);
    // A row: what it is, its pid, and the app and roles it is taken for (none: refused).
    type Row<'a> = (&'a str, u32, Option<(&'a str, &'a [CallerRole])>);
    let rows: [Row<'_>; 12] = [
        ("the shell's unit", 1, Some(("org.quire.Shell", SILL))),
        ("the shell's old scope is nobody", 2, None),
        (
            "a process of the shell's unit that is not its main process",
            3,
            None,
        ),
        (
            "companiond's unit",
            4,
            Some(("org.quire.Companion1", &[CallerRole::Companion])),
        ),
        (
            "an app scope called like companiond",
            5,
            Some(("org.quire.Companion1", &[])),
        ),
        (
            "a terminal's child",
            6,
            Some(("org.quire.Do", &[CallerRole::Cli])),
        ),
        ("a unit the table does not name", 7, None),
        (
            "the editor's scope, while the setting is on",
            8,
            Some((
                "org.quire.Acp",
                &[CallerRole::Editor, CallerRole::Companion],
            )),
        ),
        (
            "cuad's unit",
            9,
            Some(("org.quire.Cua1", &[CallerRole::Cua])),
        ),
        (
            "an app scope called like cuad",
            10,
            Some(("org.quire.Cua1", &[])),
        ),
        ("a scope nothing names", 11, None),
        ("no such process", 99, None),
    ];
    for (what, pid, expected) in rows {
        let found = found_in(root, pid, &table);
        let got = found.as_ref().and_then(|f| who(f, &on));
        let expected = expected.map(|(n, roles)| (n.to_owned(), roles.to_vec()));
        assert_eq!(got, expected, "{what}");
    }
    // The editor's scope is nobody while the setting is off.
    let editor = found_in(root, 8, &table).expect("the scope is named");
    assert_eq!(who(&editor, &AcpGate::shut()), None);
}

#[test]
fn the_users_file_cannot_make_an_app_the_shell() {
    let user = docket_dbus::callers::table_from_toml(
        "[[caller]]\napp = \"org.quire.Shell\"\nunit = \"mine.service\"\nrole = \"sheet_host\"\n\n\
         [[caller]]\napp = \"org.quire.Shell\"\nunit = \"sill.service\"\nrole = \"app\"\n",
    )
    .expect("user table");
    let table = CallerTable::layered(callers::shipped(), user);
    let root = tempfile::tempdir().expect("scratch");
    put(root.path(), 1, "mine.service", Main::Yes);
    put(root.path(), 2, "sill.service", Main::Yes);
    assert_eq!(
        found_in(root.path(), 1, &table),
        None,
        "the row was left out"
    );
    let lowered = found_in(root.path(), 2, &table).expect("the unit is still named");
    assert_eq!(
        who(&lowered, &AcpGate::shut()),
        Some(("org.quire.Shell".to_owned(), vec![])),
        "a user may lower a role"
    );
}

#[test]
fn a_flatpak_app_is_named_by_its_sandbox_metadata() {
    let root = tempfile::tempdir().expect("scratch");
    put(
        root.path(),
        42,
        "app-flatpak-org.example.Thing-5.scope",
        Main::No,
    );
    let sandbox = root.path().join("42").join("root");
    std::fs::create_dir_all(&sandbox).expect("root dir");
    std::fs::write(
        sandbox.join(".flatpak-info"),
        "[Application]\nname=org.example.Thing\n\n[Instance]\ninstance-id=5\n",
    )
    .expect("flatpak info");
    let found = found_in(root.path(), 42, &CallerTable::default()).expect("a flatpak app");
    assert_eq!(
        who(&found, &AcpGate::shut()),
        Some(("org.example.Thing".to_owned(), vec![]))
    );
}

#[test]
fn a_process_with_no_readable_cgroup_is_nobody() {
    let root = tempfile::tempdir().expect("scratch");
    let table = CallerTable::default();
    assert_eq!(found_in(root.path(), 7, &table), None, "no file");
    let at = root.path().join("8");
    std::fs::create_dir_all(&at).expect("dir");
    std::fs::write(at.join("cgroup"), "garbage\n").expect("file");
    assert_eq!(found_in(root.path(), 8, &table), None, "not a cgroup line");
}
