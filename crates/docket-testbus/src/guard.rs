//! A child process that cannot outlive its owner.

use std::io;
use std::process::{Child, Command, Stdio};

/// The watchdog: a shell that waits for the owner's PID to vanish, then kills the child's PID.
/// Both PIDs are arguments; nothing is looked up by name.
const WATCH: &str = r#"while kill -0 "$1" 2>/dev/null; do sleep 1; done; kill -9 "$2" 2>/dev/null"#;

/// A child killed by PID, and waited for, when this drops: held for a whole test, so it runs at
/// the end, on a panic and on an early return. `spawn` must be given a command that does not
/// fork away (a daemon run with `--nofork`), or the PID is not the daemon's.
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
        let child = command.spawn()?;
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
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
