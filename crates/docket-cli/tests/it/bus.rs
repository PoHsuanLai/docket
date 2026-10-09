//! The real `quire-do` binary against a private bus: a `dbus-daemon` made from a scratch config
//! in a scratch directory, a scratch HOME and runtime directory, and nothing from the
//! environment. The person's real session bus is never named, so it is never reached.
//!
//! Where intentd is not on the bus the binary exits 6 and sends nothing (this file). With an
//! intentd on it, `e2e.rs` runs the same binary against intentd's real `start`; every exit code
//! is also tested through the library against docket-fake (`exit_codes.rs`).

use docket_testbus::PrivateBus;
use std::path::Path;
use std::process::Command;

/// `quire-do` with a scratch world: only what is named here reaches the process.
fn quire_do(dir: &Path, bus: &str, words: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_quire-do"))
        .args(words)
        .env_clear()
        .env("HOME", dir)
        .env("XDG_RUNTIME_DIR", dir)
        .env("XDG_DATA_DIRS", dir)
        .env("XDG_DATA_HOME", dir)
        .env("XDG_CONFIG_HOME", dir)
        .env("DBUS_SESSION_BUS_ADDRESS", bus)
        .output()
        .expect("quire-do runs")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn where_intentd_is_not_the_exit_is_6_and_nothing_is_sent() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let bus = PrivateBus::start_eavesdropping(dir.path());
    assert!(bus.address().starts_with("unix:"), "{}", bus.address());
    assert!(
        !bus.address().contains("/run/user"),
        "the private bus is not the person's own: {}",
        bus.address()
    );
    for words in [
        vec!["apps"],
        vec!["mail", "--list"],
        vec!["mail", "thread.archive", "t1"],
        vec!["undo", "--last"],
    ] {
        let out = quire_do(dir.path(), bus.address(), &words);
        assert_eq!(
            out.status.code(),
            Some(6),
            "{words:?}: {}",
            text(&out.stderr)
        );
        assert!(
            text(&out.stderr).contains("intentd is not running"),
            "{}",
            text(&out.stderr)
        );
        assert_eq!(text(&out.stdout), "");
    }
    // Last step: a bus address that is not there at all is exit 6 too.
    let nowhere = format!("unix:path={}", dir.path().join("nothing.sock").display());
    let out = quire_do(dir.path(), &nowhere, &["apps"]);
    assert_eq!(
        out.status.code(),
        Some(6),
        "no bus at all: {}",
        text(&out.stderr)
    );
    assert!(
        text(&out.stderr).contains("the bus failed"),
        "no bus at all: {}",
        text(&out.stderr)
    );
}

#[test]
fn help_and_usage_need_no_bus_and_the_help_names_every_exit_code() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let nowhere = format!("unix:path={}", dir.path().join("nothing.sock").display());
    let help = quire_do(dir.path(), &nowhere, &["--help"]);
    assert_eq!(help.status.code(), Some(0));
    let help = text(&help.stdout);
    for code in [
        "  0  done",
        "  2  usage",
        "  3  refused by policy",
        "  4  the person declined",
        "  5  the app failed",
        "  6  unavailable",
        "  7  halted",
    ] {
        assert!(help.contains(code), "{code} in help");
    }
    assert!(help.contains("no flag that skips the question"));
    let version = quire_do(dir.path(), &nowhere, &["--version"]);
    assert_eq!(version.status.code(), Some(0));
    let usage = quire_do(dir.path(), &nowhere, &["mail", "thread.read", "--oops"]);
    assert_eq!(
        usage.status.code(),
        Some(2),
        "a command line that does not parse needs no bus"
    );
    let none = quire_do(dir.path(), &nowhere, &[]);
    assert_eq!(none.status.code(), Some(2));
}
