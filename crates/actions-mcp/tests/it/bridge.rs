//! The real `actions-mcp --host-socket` binary as an external agent starts it: MCP on its stdio,
//! one unix socket to the host, the token in its environment. The host here is a small listener
//! that records each line and answers as the real one does. The bus is unreachable, HOME and
//! every XDG directory are scratch, and the child is killed by its handle when the test ends.

use actions_tools::{EDGE_TOKEN_ENV, EdgeOp, EdgeReply, EdgeRequest, EdgeToken, EdgeTool, tool_of};
use rmcp::model::CallToolRequestParams;
use rmcp::{RoleClient, ServiceExt, service::RunningService};
use serde_json::json;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::process::{Child, Command};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

struct Bridge {
    child: Child,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

fn spawn(dir: &Path, socket: &Path, token: Option<&str>) -> Bridge {
    let mut command = Command::new(env!("CARGO_BIN_EXE_actions-mcp"));
    command
        .env_clear()
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_CONFIG_DIRS", dir.join("none"))
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/none")
        .arg("--host-socket")
        .arg(socket);
    if let Some(token) = token {
        command.env(EDGE_TOKEN_ENV, token);
    }
    let child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("actions-mcp starts");
    Bridge { child }
}

async fn client(bridge: &mut Bridge) -> RunningService<RoleClient, ()> {
    let stdin = bridge.child.stdin.take().expect("stdin");
    let stdout = bridge.child.stdout.take().expect("stdout");
    ().serve((stdout, stdin))
        .await
        .expect("the client connects")
}

type Seen = Arc<Mutex<Vec<EdgeRequest>>>;

/// A host that answers `reply` to every well-formed line and records the requests.
fn host(socket: &Path, reply: impl Fn(&EdgeRequest) -> EdgeReply + Send + Sync + 'static) -> Seen {
    let listener = UnixListener::bind(socket).expect("socket");
    let seen = Seen::default();
    let record = seen.clone();
    let reply = Arc::new(reply);
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let (record, reply) = (record.clone(), reply.clone());
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut lines = BufReader::new(read).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let Ok(request) = serde_json::from_str::<EdgeRequest>(&line) else {
                        return;
                    };
                    let answer = reply(&request);
                    record.lock().expect("lock").push(request);
                    let text = format!("{}\n", serde_json::to_string(&answer).expect("json"));
                    if write.write_all(text.as_bytes()).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    seen
}

fn mail_tools() -> Vec<EdgeTool> {
    let manifest = docket_fake::mail_manifest().expect("manifest");
    let registry = [manifest];
    actions_tools::offered(&registry)
        .into_iter()
        .filter_map(|(m, a)| tool_of(m, a))
        .map(|(tool, description)| EdgeTool { tool, description })
        .collect()
}

#[tokio::test]
async fn the_list_says_it_is_private_and_never_fresh() {
    // A 2026-07-28 client (Claude Code's) refuses a tool list without these two fields, and then
    // the agent has none of the desktop's tools.
    let dir = tempfile::tempdir().expect("scratch");
    let socket = dir.path().join("host.sock");
    let _seen = host(&socket, |_| EdgeReply::Tools(mail_tools()));
    let mut bridge = spawn(dir.path(), &socket, Some(TOKEN));
    let client = client(&mut bridge).await;
    let listed = client.list_tools(None).await.expect("tools");
    assert_eq!(listed.ttl_ms, Some(0));
    assert_eq!(listed.cache_scope, Some(rmcp::model::CacheScope::Private));
    assert!(!listed.tools.is_empty());
}

#[tokio::test]
async fn the_bridge_lists_what_the_host_lists_and_sends_the_token_and_nothing_else() {
    let dir = tempfile::tempdir().expect("scratch");
    let socket = dir.path().join("host.sock");
    let seen = host(&socket, |_| EdgeReply::Tools(mail_tools()));
    let mut bridge = spawn(dir.path(), &socket, Some(TOKEN));
    let client = client(&mut bridge).await;
    let listed = client.list_all_tools().await.expect("tools");
    let names: Vec<String> = listed.iter().map(|t| t.name.to_string()).collect();
    assert!(
        names.contains(&"mail__mail_thread_read".to_owned()),
        "{names:?}"
    );
    let requests = seen.lock().expect("lock").clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].op, EdgeOp::List);
    assert!(
        requests[0]
            .token
            .matches(&EdgeToken::parse(TOKEN).expect("token"))
    );
}

#[tokio::test]
async fn a_call_is_one_line_with_the_tool_and_the_arguments_as_given() {
    let dir = tempfile::tempdir().expect("scratch");
    let socket = dir.path().join("host.sock");
    let seen = host(&socket, |_| {
        EdgeReply::Done(json!({"said": "ok", "value": null}))
    });
    let mut bridge = spawn(dir.path(), &socket, Some(TOKEN));
    let client = client(&mut bridge).await;
    let arguments =
        json!({"target": {"app": "org.quire.Mail", "kind": "mail.thread", "key": "t1"}});
    let params = CallToolRequestParams::new("mail__mail_thread_read")
        .with_arguments(arguments.as_object().cloned().expect("object"));
    let result = client.call_tool(params).await.expect("call");
    assert_ne!(result.is_error, Some(true));
    let requests = seen.lock().expect("lock").clone();
    match &requests[0].op {
        EdgeOp::Call {
            tool,
            arguments: sent,
        } => {
            assert_eq!(tool, "mail__mail_thread_read");
            assert_eq!(sent, &arguments);
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_refused_line_or_a_gone_host_is_a_tool_error_and_never_a_success() {
    let dir = tempfile::tempdir().expect("scratch");
    let socket = dir.path().join("host.sock");
    let _seen = host(&socket, |_| EdgeReply::Refused);
    let mut bridge = spawn(dir.path(), &socket, Some(TOKEN));
    let client = client(&mut bridge).await;
    let call = || CallToolRequestParams::new("mail__mail_thread_read");
    assert_eq!(
        client.call_tool(call()).await.expect("answer").is_error,
        Some(true)
    );
    assert!(client.list_all_tools().await.expect("tools").is_empty());
    std::fs::remove_file(&socket).expect("socket gone");
    assert_eq!(
        client.call_tool(call()).await.expect("answer").is_error,
        Some(true)
    );
}

#[tokio::test]
async fn without_a_session_token_the_bridge_does_not_start() {
    let dir = tempfile::tempdir().expect("scratch");
    let socket = dir.path().join("host.sock");
    for token in [None, Some("short"), Some(&"G".repeat(64)[..])] {
        let mut bridge = spawn(dir.path(), &socket, token);
        let status = bridge.child.wait().await.expect("exit");
        assert_eq!(status.code(), Some(1), "{token:?}");
    }
}
