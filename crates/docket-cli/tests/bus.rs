//! The real `quire-do` binary against a private bus: a `dbus-daemon` made from a scratch config
//! in a scratch directory, a scratch HOME and runtime directory, and nothing from the
//! environment. The person's real session bus is never named, so it is never reached.
//!
//! Where intentd is not on the bus the binary exits 6 and sends nothing (this file). With an
//! intentd on it, `e2e.rs` runs the same binary against intentd's real `start`; every exit code
//! is also tested through the library against docket-fake (`exit_codes.rs`).

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};

const CONFIG: &str = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path=SOCKET</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#;

/// A bus of our own, killed when the test ends.
struct PrivateBus {
    child: Child,
    address: String,
}

impl PrivateBus {
    fn start(dir: &Path) -> Option<Self> {
        let socket = dir.join("bus.sock");
        let config = dir.join("bus.conf");
        std::fs::write(&config, CONFIG.replace("SOCKET", &socket.to_string_lossy())).ok()?;
        let mut child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .env_clear()
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut address = String::new();
        BufReader::new(child.stdout.take()?)
            .read_line(&mut address)
            .ok()?;
        Some(Self {
            child,
            address: address.trim().to_owned(),
        })
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

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
    let Some(bus) = PrivateBus::start(dir.path()) else {
        panic!("dbus-daemon is needed for the private-bus tests (the gate machine has it)");
    };
    assert!(bus.address.starts_with("unix:"), "{}", bus.address);
    assert!(
        !bus.address.contains("/run/user"),
        "the private bus is not the person's own: {}",
        bus.address
    );
    for words in [
        vec!["apps"],
        vec!["mail", "--list"],
        vec!["mail", "thread.archive", "t1"],
        vec!["undo", "--last"],
    ] {
        let out = quire_do(dir.path(), &bus.address, &words);
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
}

#[test]
fn a_bus_that_is_not_there_is_exit_6_too() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let nowhere = format!("unix:path={}", dir.path().join("nothing.sock").display());
    let out = quire_do(dir.path(), &nowhere, &["apps"]);
    assert_eq!(out.status.code(), Some(6), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("the bus failed"),
        "{}",
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
