//! Who holds a standing: the daemon's table over a fake proc root, as `Identity` reads it for a
//! connection. Rows: the right unit, a scope that claims the role, a user's row that claims it, a
//! process that is not the unit's main process, and a process nothing names.

use docket_dbus::callers::{INTENTD, SHELL, Standing, config_home, load_callers, table_from_toml};
use porter_dbus::callers::{CallerTable, ProcCallers};
use std::path::Path;

const SYSTEM: &str = r#"
[[caller]]
app = "org.quire.Intents"
unit = "intentd.service"
role = "agent"

[[caller]]
app = "org.quire.Shell"
unit = "sill.service"
role = "sheet_host"

# A scope never carries a fixed role: this row is the shell's name as a plain app.
[[caller]]
app = "org.quire.Shell"
unit = "sill-shell.scope"
role = "sheet_host"
"#;

/// A process `pid` in the cgroup `leaf`; a service's process is also its main process.
fn put(root: &Path, pid: u32, leaf: &str, main: bool) {
    let dir = root.join(pid.to_string());
    std::fs::create_dir_all(&dir).expect("pid dir");
    let slice = "0::/user.slice/user-1000.slice/user@1000.service/app.slice";
    std::fs::write(dir.join("cgroup"), format!("{slice}/{leaf}\n")).expect("cgroup");
    if main {
        let units = root.join("units");
        std::fs::create_dir_all(&units).expect("units dir");
        std::fs::write(units.join(leaf), pid.to_string()).expect("main pid");
    }
}

fn holds(root: &Path, pid: u32, table: &CallerTable, standing: Standing) -> bool {
    ProcCallers::caller_of_pid(root, pid, table).is_some_and(|c| standing.held_by(&c))
}

#[test]
fn a_standing_is_held_by_the_unit_and_by_nothing_that_only_claims_it() {
    let root = tempfile::tempdir().expect("scratch");
    let root = root.path();
    put(root, 1, "intentd.service", true);
    put(root, 2, "sill.service", true);
    put(root, 3, "sill-shell.scope", false);
    put(root, 4, "intentd.service", false);
    put(root, 5, "app-org.quire.Intents-1.scope", false);
    put(root, 6, "curl.service", true);
    put(root, 7, "vte-spawn-1.scope", false);
    let table = table_from_toml(SYSTEM).expect("table");
    // (what it is, pid, the router's standing, the shell's standing)
    for (what, pid, router, shell) in [
        ("the router's unit", 1, true, false),
        ("the shell's unit", 2, false, true),
        ("a scope that claims the shell's role", 3, false, false),
        (
            "a process of the router's unit that is not its main process",
            4,
            false,
            false,
        ),
        ("an app scope called like the router", 5, false, false),
        ("a unit the table does not name", 6, false, false),
        ("a terminal's child", 7, false, false),
        ("no such process", 99, false, false),
    ] {
        assert_eq!(holds(root, pid, &table, INTENTD), router, "{what}: router");
        assert_eq!(holds(root, pid, &table, SHELL), shell, "{what}: shell");
    }
    assert!(
        !holds(root, 1, &CallerTable::default(), INTENTD),
        "an empty table names nobody"
    );
}

#[test]
fn the_users_file_cannot_give_a_fixed_role_and_can_lower_one() {
    let dir = tempfile::tempdir().expect("scratch");
    let (system, user) = (dir.path().join("system.toml"), dir.path().join("user.toml"));
    std::fs::write(&system, SYSTEM).expect("system");
    std::fs::write(
        &user,
        "[[caller]]\napp = \"org.quire.Shell\"\nunit = \"other.service\"\nrole = \"sheet_host\"\n\n\
         [[caller]]\napp = \"org.example.Mine\"\nunit = \"intentd.service\"\nrole = \"app\"\n",
    )
    .expect("user");
    let table = load_callers(&system, &user).expect("layered");
    let root = tempfile::tempdir().expect("scratch");
    put(root.path(), 1, "other.service", true);
    put(root.path(), 2, "intentd.service", true);
    assert!(
        !holds(root.path(), 1, &table, SHELL),
        "the user's sheet_host row is left out"
    );
    assert!(
        !holds(root.path(), 2, &table, INTENTD),
        "the user's row lowered the router's unit"
    );
}

#[test]
fn the_configuration_directory_follows_the_environment() {
    let env = |pairs: &'static [(&'static str, &'static str)]| {
        move |key: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_owned())
        }
    };
    assert_eq!(
        config_home(&env(&[("XDG_CONFIG_HOME", "/x/cfg")])),
        Path::new("/x/cfg")
    );
    assert_eq!(
        config_home(&env(&[("HOME", "/home/p")])),
        Path::new("/home/p/.config")
    );
    assert_eq!(
        config_home(&env(&[("XDG_CONFIG_HOME", ""), ("HOME", "/h")])),
        Path::new("/h/.config")
    );
}
