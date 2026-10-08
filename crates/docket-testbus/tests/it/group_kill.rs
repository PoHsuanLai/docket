//! A test process killed without unwinding (a timeout's SIGKILL, sent to its whole group) leaves
//! no bus and no service behind. The test re-runs this executable as the doomed process.

use docket_testbus::{PrivateBus, Reaped};
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

/// Set in the doomed process's environment; its presence makes `doomed_process` do the work.
const DOOMED: &str = "DOCKET_TESTBUS_DOOMED";
/// How many 10 ms polls a process gets to disappear. A counted bound, not a timing assertion.
const POLLS: usize = 1000;

/// The doomed process: a private bus and a service, announced on stdout, held until killed.
/// Does nothing in a normal run.
#[test]
fn doomed_process() {
    let Ok(dir) = std::env::var(DOOMED) else {
        return;
    };
    let bus = PrivateBus::start(Path::new(&dir));
    let service = Reaped::spawn(Command::new("/bin/sleep").arg("600")).expect("service starts");
    println!("PIDS {} {}", bus.pid(), service.pid());
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}

fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| !stat.contains(") Z "))
}

fn pids_of(line: &str) -> Vec<u32> {
    line.strip_prefix("PIDS ")
        .map(|rest| {
            rest.split_whitespace()
                .filter_map(|p| p.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_killed_test_group_leaves_no_bus_and_no_service() {
    let dir = tempfile::tempdir().expect("scratch dir");
    let mut child = Command::new(std::env::current_exe().expect("test binary"))
        .args(["group_kill::doomed_process", "--exact", "--nocapture"])
        .env_clear()
        .env(DOOMED, dir.path())
        .env("HOME", dir.path())
        .env("XDG_RUNTIME_DIR", dir.path())
        .env("PATH", "/usr/bin:/bin")
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("doomed process starts");
    let stdout = child.stdout.take().expect("stdout");
    let pids = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .map(|line| pids_of(&line))
        .find(|pids| !pids.is_empty())
        .expect("the doomed process announced its pids");
    assert!(pids.iter().all(|p| alive(*p)), "{pids:?} never started");
    // What a timeout does: SIGKILL to the whole group, the watchdog's included.
    let group = format!("-{}", child.id());
    let status = Command::new("/bin/kill")
        .args(["-KILL", "--", &group])
        .status()
        .expect("kill runs");
    assert!(status.success());
    let _ = child.wait();
    let gone = (0..POLLS).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(10));
        pids.iter().all(|p| !alive(*p))
    });
    let left: Vec<u32> = pids.iter().copied().filter(|p| alive(*p)).collect();
    for pid in &left {
        let _ = Command::new("/bin/kill")
            .args(["-KILL", &pid.to_string()])
            .status();
    }
    assert!(gone, "outlived a killed test group: {left:?}");
}
