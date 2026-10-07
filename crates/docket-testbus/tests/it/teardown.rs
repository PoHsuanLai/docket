//! Teardown is certain: the daemon is gone after the guard drops, after a panic, and after the
//! owning process is killed outright. Every check goes by PID; nothing is matched by name.

use docket_testbus::{PrivateBus, Reaped};
use std::io::{BufRead, BufReader};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The command line of a live process, `None` once it is gone (or only a zombie).
fn cmdline(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let state = stat.rsplit(") ").next()?.chars().next()?;
    if state == 'Z' {
        return None;
    }
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    Some(String::from_utf8_lossy(&raw).replace('\0', " "))
}

fn gone_within(pid: u32, wait: Duration) -> bool {
    let end = Instant::now() + wait;
    while Instant::now() < end {
        if cmdline(pid).is_none() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    cmdline(pid).is_none()
}

#[test]
fn the_pid_is_the_daemons_and_dropping_the_bus_ends_it() {
    let dir = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(dir.path());
    let pid = bus.pid();
    let line = cmdline(pid).expect("the daemon is running");
    assert!(line.starts_with("dbus-daemon "), "not the daemon: {line}");
    assert!(line.contains(&dir.path().display().to_string()), "{line}");
    drop(bus);
    assert!(cmdline(pid).is_none(), "the daemon outlived its bus");
}

#[test]
fn a_panic_in_the_test_still_ends_the_daemon() {
    let dir = tempfile::tempdir().expect("scratch");
    let mut seen = 0;
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let bus = PrivateBus::start(dir.path());
        seen = bus.pid();
        panic!("the test fails with the bus held");
    }));
    assert!(outcome.is_err());
    assert_ne!(seen, 0);
    assert!(cmdline(seen).is_none(), "the daemon outlived a panic");
}

#[test]
fn a_guard_made_before_a_later_failure_ends_its_child() {
    let mut seen = 0;
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let child = Reaped::spawn(Command::new("sleep").arg("600").stdout(Stdio::null()))
            .expect("sleep starts");
        seen = child.pid();
        assert!(cmdline(seen).is_some());
        panic!("a failure after the spawn");
    }));
    assert!(outcome.is_err());
    assert!(cmdline(seen).is_none(), "the child outlived a panic");
}

#[test]
fn a_killed_test_process_does_not_leave_its_daemon() {
    if std::env::var_os("TESTBUS_ORPHAN_DIR").is_some() {
        return;
    }
    let dir = tempfile::tempdir().expect("scratch");
    // Guarded too, so a failed assertion below does not leave the owner behind.
    let mut owner = Reaped::spawn(
        Command::new(std::env::current_exe().expect("this test binary"))
            .args([
                "teardown::orphaned_owner",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("TESTBUS_ORPHAN_DIR", dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()),
    )
    .expect("the owner starts");
    let stdout = owner.child_mut().stdout.take().expect("stdout");
    let mut lines = BufReader::new(stdout).lines();
    let pid: u32 = lines
        .find_map(|line| {
            // libtest prefixes the first line of a test's output with `test <name> ... `.
            line.ok()?.rsplit("DAEMON_PID=").next()?.trim().parse().ok()
        })
        .expect("the owner names its daemon");
    assert!(cmdline(pid).is_some(), "the daemon is running");
    // SIGKILL: no unwinding, no `Drop`.
    owner.child_mut().kill().expect("kill the owner");
    owner.child_mut().wait().expect("wait for the owner");
    assert!(
        gone_within(pid, Duration::from_secs(10)),
        "the watchdog left the daemon running"
    );
}

/// Run by the test above as a separate process: holds a bus and waits to be killed.
#[test]
fn orphaned_owner() {
    let Some(dir) = std::env::var_os("TESTBUS_ORPHAN_DIR").map(PathBuf::from) else {
        return;
    };
    let bus = PrivateBus::start(&dir);
    println!("DAEMON_PID={}", bus.pid());
    std::thread::sleep(Duration::from_secs(120));
}
