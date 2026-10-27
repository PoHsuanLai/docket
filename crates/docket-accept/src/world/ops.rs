//! What a running world does: connections, the model's audit, `quire-do`, the daemons' logs.

use super::{Cgroup, World, env_of, place};
use docket_testbus::Reaped;
use std::path::Path;
use std::process::{Command, Stdio};

impl World {
    /// A fresh connection to the private bus.
    pub async fn connect(&self) -> zbus::Connection {
        self.bus.connect().await
    }

    /// A connection to the bus that is a process of its own, placed in `cgroup`: through
    /// `accept-bus-relay`, so the daemons identify it apart from the harness process (which is
    /// the shell). An agent host the harness runs in-process connects this way.
    pub async fn connect_as(&self, relay: &Path, cgroup: Cgroup<'_>) -> zbus::Connection {
        let socket = self
            .address()
            .strip_prefix("unix:path=")
            .and_then(|rest| rest.split(',').next())
            .expect("the private bus listens on a path");
        let (ours, theirs) = std::os::unix::net::UnixStream::pair().expect("a socket pair");
        let relayed = Reaped::spawn(
            Command::new(relay)
                .arg(socket)
                .env_clear()
                .stdin(Stdio::from(std::os::fd::OwnedFd::from(theirs)))
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        )
        .expect("the relay starts");
        place(self.dir.path(), relayed.pid(), cgroup);
        self.relays
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(relayed);
        ours.set_nonblocking(true).expect("non-blocking");
        let stream = tokio::net::UnixStream::from_std(ours).expect("a tokio stream");
        zbus::connection::Builder::unix_stream(stream)
            .build()
            .await
            .expect("connect through the relay")
    }

    /// The bus address.
    pub fn address(&self) -> &str {
        self.bus.address()
    }

    /// inferd's audit trail so far: one entry per finished model turn, with the app that asked
    /// (never the content).
    pub fn model_turns(&self) -> Vec<porter_infer::AuditEntry> {
        std::fs::read_to_string(
            self.dir
                .path()
                .join(".local/state/quire/inferd/audit.jsonl"),
        )
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
    }

    /// The requests inferd's replay engine was asked, one JSON line each, when the run was started
    /// with `Options::record` on; empty otherwise.
    pub fn recorded_requests(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("record.jsonl")).unwrap_or_default()
    }

    /// The real `quire-do` (`binary`: `accept-quire-do`) with `words`, run as a terminal's child:
    /// it starts through `sh`, which waits for a line on stdin, so its pid (kept by `exec`) is
    /// placed in a `vte-spawn-*` scope of the scratch proc root before the program exists. Its
    /// environment is the scratch one and the private bus, nothing else.
    pub async fn quire_do(&self, binary: &Path, words: &[&str]) -> std::process::Output {
        use std::io::{Read, Write};
        let mut process = Reaped::spawn(
            Command::new("sh")
                .args(["-c", "read _; exec \"$0\" \"$@\" </dev/null"])
                .arg(binary)
                .args(words)
                .env_clear()
                .envs(env_of(self.dir.path()))
                .env("DBUS_SESSION_BUS_ADDRESS", self.address())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .expect("quire-do starts");
        place(self.dir.path(), process.pid(), Cgroup::Scope("vte-spawn-1"));
        let child = process.child_mut();
        let mut go = child.stdin.take().expect("stdin");
        go.write_all(b"\n").expect("go");
        drop(go);
        let (mut out, mut err) = (
            child.stdout.take().expect("stdout"),
            child.stderr.take().expect("stderr"),
        );
        // The guard travels into the blocking task, so the program is killed by PID if the test
        // is dropped while it runs.
        tokio::task::spawn_blocking(move || {
            let reader = std::thread::spawn(move || {
                let mut bytes = Vec::new();
                err.read_to_end(&mut bytes).map(|_| bytes)
            });
            let mut stdout = Vec::new();
            out.read_to_end(&mut stdout).expect("quire-do runs");
            let stderr = reader.join().expect("joined").expect("quire-do runs");
            let status = process.child_mut().wait().expect("quire-do runs");
            std::process::Output {
                status,
                stdout,
                stderr,
            }
        })
        .await
        .expect("joined")
    }

    /// Every daemon's standard error, for a failing test to print.
    pub fn logs(&self) -> String {
        self.daemons
            .iter()
            .map(|(name, _)| {
                let text =
                    std::fs::read_to_string(self.dir.path().join(format!("logs/{name}.log")))
                        .unwrap_or_default();
                format!("---- {name}\n{text}")
            })
            .collect()
    }
}
