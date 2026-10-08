//! The editor's side of an ACP run: `docket-acp` as a process on the private bus, spoken to over
//! its stdin and stdout by a scripted peer. The process is placed in the fake proc root as the
//! unit companiond runs in, which is how inferd and memoryd know it; intentd knows it by the bus
//! name it owns (`org.quire.Acp`, an editor and a companion in the shipped `intentd.toml`).

use crate::world::{Cgroup, GIVE_UP, World, env_of, place};
use docket_testbus::Reaped;
use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command, Stdio};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{ChildStdin, ChildStdout};

/// A scripted editor on a running `docket-acp`.
pub struct AcpEditor {
    // Declared first: the process goes before its pipes.
    _process: Reaped,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    next: i64,
    /// Every message the server sent, in order.
    pub seen: Vec<Value>,
}

impl std::fmt::Debug for AcpEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AcpEditor")
    }
}

impl AcpEditor {
    /// Starts `binary` (`accept-acp`) on `world`'s bus.
    pub async fn start(world: &World, binary: &Path) -> AcpEditor {
        let dir = world.dir.path();
        let mut command = Command::new(binary);
        command
            .env_clear()
            .envs(env_of(dir))
            .env("DBUS_SESSION_BUS_ADDRESS", world.address())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(
                std::fs::File::create(dir.join("logs/docket-acp.log")).expect("log"),
            ));
        let mut process = Reaped::spawn(&mut command).expect("docket-acp starts");
        place(dir, process.pid(), Cgroup::Unit("companiond"));
        let child = process.child_mut();
        let stdin = ChildStdin::from_std(child.stdin.take().expect("stdin")).expect("stdin");
        let stdout = ChildStdout::from_std(child.stdout.take().expect("stdout")).expect("stdout");
        AcpEditor {
            _process: process,
            stdin,
            lines: BufReader::new(stdout).lines(),
            next: 0,
            seen: Vec::new(),
        }
    }

    async fn write(&mut self, body: Value) {
        let mut line = body.to_string();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await.expect("write");
    }

    async fn read(&mut self) -> Value {
        let line = tokio::time::timeout(GIVE_UP, self.lines.next_line())
            .await
            .expect("docket-acp answered in time")
            .expect("read")
            .expect("the server closed");
        let message: Value = serde_json::from_str(&line).expect("json");
        self.seen.push(message.clone());
        message
    }

    /// Sends a notification.
    pub async fn notify(&mut self, method: &str, params: Value) {
        self.write(json!({"jsonrpc": "2.0", "method": method, "params": params}))
            .await;
    }

    /// Sends a request and reads until its reply; each request the server makes meanwhile is
    /// answered with `answer(request)` (nothing: it is left unanswered).
    pub async fn request(
        &mut self,
        method: &str,
        params: Value,
        answer: &mut dyn FnMut(&Value) -> Option<Value>,
    ) -> Value {
        self.next += 1;
        let id = self.next;
        self.write(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        loop {
            let message = self.read().await;
            if message["method"] == "session/request_permission" {
                if let Some(result) = answer(&message) {
                    self.write(json!({"jsonrpc": "2.0", "id": message["id"], "result": result}))
                        .await;
                }
            } else if message["id"] == json!(id) {
                return message;
            }
        }
    }

    /// The `session/update` payloads seen so far, oldest first.
    pub fn updates(&self) -> Vec<Value> {
        self.seen
            .iter()
            .filter(|m| m["method"] == "session/update")
            .map(|m| m["params"]["update"].clone())
            .collect()
    }
}
