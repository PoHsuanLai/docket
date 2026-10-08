//! A child process that cannot outlive its owner.

use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};

/// The watchdog: a shell that waits for the owner's PID to vanish, then kills the child's process
/// group. Both ids are arguments; nothing is looked up by name.
const WATCH: &str =
    r#"while kill -0 "$1" 2>/dev/null; do sleep 1; done; kill -9 -- "-$2" 2>/dev/null"#;

/// A child killed by PID, and waited for, when this drops: held for a whole test, so it runs at
/// the end, on a panic and on an early return. `spawn` must be given a command that does not
/// fork away (a daemon run with `--nofork`), or the PID is not the daemon's.
///
/// The child leads a process group of its own. On drop the group gets SIGTERM and a grace period
/// (a daemon that put its own children in groups of their own, as inferd does with its engines,
/// ends them), then SIGKILL: nothing it started holds the GPU after the test.
#[derive(Debug)]
pub struct Reaped {
    // Declared first, so it goes first: a watchdog must never outlive the child it would kill,
    // or a reused PID could be its victim.
    watchdog: Option<Child>,
    child: Child,
}

impl Reaped {
    /// Starts `command` and guards it, with a watchdog for the owner dying without unwinding.
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        let child = command.process_group(0).spawn()?;
        let watchdog = Command::new("/bin/sh")
            .args(["-c", WATCH, "sh"])
            .arg(std::process::id().to_string())
            .arg(child.id().to_string())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok();
        Ok(Self { watchdog, child })
    }

    /// The child's PID.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// The child, for its pipes.
    pub fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

impl Drop for Reaped {
    fn drop(&mut self) {
        if let Some(watchdog) = self.watchdog.as_mut() {
            let _ = watchdog.kill();
            let _ = watchdog.wait();
        }
        // The groups its children lead (inferd's engines lead theirs), read before anything is
        // signalled: a daemon that dies on SIGTERM without ending them would orphan them.
        let theirs = crate::groups::groups_below(self.child.id(), &crate::groups::table());
        // SIGTERM first, so the daemon can end its own children; then SIGKILL to whatever of
        // ours is left, the daemon's group and every group found below it.
        let _ = signal_group("-TERM", self.child.id());
        let deadline = std::time::Instant::now() + GRACE;
        while std::time::Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = signal_group("-KILL", self.child.id());
        for group in theirs {
            let _ = signal_group("-KILL", group);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// How long a daemon gets to end its own children after SIGTERM (inferd gives an engine 5 s).
const GRACE: std::time::Duration = std::time::Duration::from_secs(8);

/// `signal` to the process group `pgid` leads (its id is the child's PID), by id.
fn signal_group(signal: &str, pgid: u32) -> io::Result<std::process::ExitStatus> {
    Command::new("/bin/kill")
        .args([signal, "--", &format!("-{pgid}")])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The process group `pid` is in, from `/proc/<pid>/stat` (the fields after the name).
    fn group_of(pid: u32) -> Option<u32> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let after = stat.rsplit_once(')')?.1;
        after.split_whitespace().nth(2)?.parse().ok()
    }

    #[test]
    fn a_grandchild_in_a_group_of_its_own_goes_too() {
        let dir = tempfile::tempdir().expect("dir");
        let pidfile = dir.path().join("grandchild");
        // `setsid` puts the sleeper in a session and group of its own, as inferd does an engine;
        // `trap '' TERM` keeps the shell from ending it, as a daemon dying on SIGTERM would not.
        let script = format!(
            "trap '' TERM; /usr/bin/setsid /bin/sleep 300 & echo $! > {}; wait",
            pidfile.display()
        );
        let guard = Reaped::spawn(Command::new("/bin/sh").args(["-c", &script])).expect("spawn");
        let started = (0..200).find_map(|_| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            std::fs::read_to_string(&pidfile)
                .ok()
                .and_then(|t| t.trim().parse::<u32>().ok())
        });
        let grandchild = started.expect("the grandchild started");
        // The pid is written before `setsid` has run: dropping now races the move into a group of
        // its own, and a group read before it and signalled after it misses the sleeper.
        let own_group = (0..200).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            group_of(grandchild) == Some(grandchild)
        });
        assert!(own_group, "the grandchild never led a group of its own");
        drop(guard);
        let gone = (0..200).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            !std::path::Path::new(&format!("/proc/{grandchild}")).exists()
                || std::fs::read_to_string(format!("/proc/{grandchild}/stat"))
                    .unwrap_or_default()
                    .contains(") Z ")
        });
        assert!(gone, "grandchild {grandchild} outlived the guard");
    }

    #[test]
    fn a_grandchild_goes_with_the_child() {
        let dir = tempfile::tempdir().expect("dir");
        let pidfile = dir.path().join("grandchild");
        let script = format!("sleep 300 & echo $! > {}; wait", pidfile.display());
        let guard = Reaped::spawn(Command::new("/bin/sh").args(["-c", &script])).expect("spawn");
        let grandchild = (0..200)
            .find_map(|_| {
                std::thread::sleep(std::time::Duration::from_millis(10));
                std::fs::read_to_string(&pidfile)
                    .ok()
                    .and_then(|t| t.trim().parse::<u32>().ok())
            })
            .expect("the grandchild started");
        drop(guard);
        let alive = |pid: u32| {
            std::path::Path::new(&format!("/proc/{pid}")).exists()
                && !std::fs::read_to_string(format!("/proc/{pid}/stat"))
                    .unwrap_or_default()
                    .contains(") Z ")
        };
        let gone = (0..200).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            !alive(grandchild)
        });
        assert!(gone, "grandchild {grandchild} outlived the guard");
    }
}
