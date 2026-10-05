//! The real `actions-mcp` binary on a private bus: intentd's own `serve_on` behind the fake router
//! and the shipped roles, the binary over stdio (an rmcp client on its pipes) and over a Unix
//! socket. Nothing of the real session is named: the bus, HOME and every XDG directory are scratch,
//! the daemon's environment is cleared, and the child is killed when the test ends.

use actions_mcp::McpConfig;
use docket_core::*;
use docket_fake::{MailThread, fake_router};
use docket_router::GrantStore;
use docket_testbus::PrivateBus;
use intentd::{IntentdConfig, serve_on};
use porter_core::AppName;
use porter_core::consent::{Decision, Grant, GrantScope, Usage};
use prov::{ClientName, DataClass, SpaceId, SpaceScope, UnixSeconds};
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::{RoleClient, ServiceExt, service::RunningService};
use serde_json::{Value as Json, json};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::AsyncBufReadExt;
use tokio::process::{Child, Command};

const CLIENT: &str = "org.example.ClaudeDesktop";

fn router() -> docket_router::Router<docket_fake::FakeSeams> {
    let router = fake_router(AgentConfig::default()).expect("router");
    router.seams.link.mail.add_thread(MailThread {
        key: "t1".into(),
        subject: "Invoice".into(),
        from: "eve@evil.test".into(),
        body: "Pay now".into(),
    });
    // The router names an MCP caller by the bus name its connection owns, which is the edge
    // process, not the client it speaks for (the client name is what labels the arguments).
    let client = ClientName::parse(actions_mcp::MCP_BUS).expect("client");
    for (n, usage) in [Usage::Interactive, Usage::Background]
        .into_iter()
        .enumerate()
    {
        router.seams.grants.record(Grant {
            id: porter_core::GrantId::parse(&format!("g-{n}")).expect("grant"),
            key: ActionGrantKey {
                caller: GrantCaller::Mcp(client.clone()),
                owner: AppName::parse("org.quire.Mail").expect("app"),
                target: GrantTarget::App,
                class: DataClass::Mail,
                usage,
                space: SpaceScope::Only(SpaceId::desktop()),
            },
            decision: Decision::Allow,
            scope: GrantScope::Always,
            at: UnixSeconds(0),
        });
    }
    router
}

/// A child that is killed (by its handle, so by PID) when the test ends.
struct Edge {
    child: Child,
}

impl Drop for Edge {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

fn spawn(dir: &Path, bus: &str, extra: &[&str]) -> Edge {
    let child = Command::new(env!("CARGO_BIN_EXE_actions-mcp"))
        .env_clear()
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_CONFIG_DIRS", dir.join("none"))
        .env("DBUS_SESSION_BUS_ADDRESS", bus)
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("actions-mcp starts");
    Edge { child }
}

fn configure(dir: &Path, text: &str) {
    let config = dir.join("config/quire");
    std::fs::create_dir_all(&config).expect("dirs");
    std::fs::write(config.join("actions-mcp.toml"), text).expect("config");
}

/// The person switches the edge on in docket's settings.
fn switch_on(dir: &Path) {
    let config = dir.join("config/docket");
    std::fs::create_dir_all(&config).expect("dirs");
    std::fs::write(
        config.join("settings.toml"),
        "[agent.mcp]\nexpose = \"on\"\n",
    )
    .expect("settings");
}

async fn client_over_stdio(edge: &mut Edge) -> RunningService<RoleClient, ()> {
    let stdin = edge.child.stdin.take().expect("stdin");
    let stdout = edge.child.stdout.take().expect("stdout");
    ().serve((stdout, stdin))
        .await
        .expect("the client connects")
}

async fn read_thread(client: &RunningService<RoleClient, ()>) -> CallToolResult {
    let target = json!({ "app": "org.quire.Mail", "kind": "mail.thread", "key": "t1" });
    let Json::Object(arguments) = json!({ "target": target }) else {
        unreachable!()
    };
    client
        .call_tool(
            CallToolRequestParams::new("mail__mail_thread_read".to_owned())
                .with_arguments(arguments),
        )
        .await
        .expect("a tool result")
}

async fn desk() -> (tempfile::TempDir, PrivateBus, docket_dbus::BusConnection) {
    let dir = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(dir.path());
    let daemon = bus.connect().await;
    serve_on(
        &daemon,
        Arc::new(router()),
        Arc::new(IntentdConfig::shipped().expect("shipped")),
    )
    .await
    .expect("intentd serves");
    (dir, bus, daemon)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_stdio_a_switched_on_edge_serves_the_registry_as_the_mcp_role() {
    let (dir, bus, _intentd) = desk().await;
    switch_on(dir.path());
    configure(dir.path(), &format!("client = \"{CLIENT}\"\n"));
    let mut edge = spawn(dir.path(), bus.address(), &[]);
    let client = client_over_stdio(&mut edge).await;
    let names: Vec<String> = client
        .list_all_tools()
        .await
        .expect("list")
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(
        names.contains(&"mail__mail_thread_read".to_owned()),
        "{names:?}"
    );
    // A read is allowed for the role the bus name gives: it reached intentd as Actor::Mcp.
    let result = read_thread(&client).await;
    assert_eq!(result.is_error, Some(false), "{result:?}");
    // A second edge finds the bus name taken (several clients share one edge through the socket).
    let mut other = spawn(dir.path(), bus.address(), &["--client", "someone-else"]);
    let stdin = other.child.stdin.take().expect("stdin");
    let stdout = other.child.stdout.take().expect("stdout");
    // Its bus name is the first edge's, so it never serves: it stops and the handshake fails.
    assert!(().serve((stdout, stdin)).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_no_configuration_the_edge_is_off_and_lists_nothing() {
    let (dir, bus, _intentd) = desk().await;
    let mut edge = spawn(dir.path(), bus.address(), &[]);
    let client = client_over_stdio(&mut edge).await;
    assert!(client.list_all_tools().await.expect("list").is_empty());
    let result = read_thread(&client).await;
    assert_eq!(result.is_error, Some(true));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_a_unix_socket_many_clients_share_one_edge() {
    let (dir, bus, _intentd) = desk().await;
    switch_on(dir.path());
    configure(dir.path(), &format!("client = \"{CLIENT}\"\n"));
    let socket = dir.path().join("mcp.sock");
    let mut edge = spawn(
        dir.path(),
        bus.address(),
        &["--socket", socket.to_str().expect("utf-8")],
    );
    // The edge says so on stderr once the socket is bound: wait for that line, not for a clock.
    let stderr = edge.child.stderr.take().expect("stderr");
    let mut lines = tokio::io::BufReader::new(stderr).lines();
    loop {
        let line = lines
            .next_line()
            .await
            .expect("stderr")
            .expect("it listens");
        if line.contains("listening") {
            break;
        }
    }
    let first = ()
        .serve(
            tokio::net::UnixStream::connect(&socket)
                .await
                .expect("connect"),
        )
        .await
        .expect("first");
    let second = ()
        .serve(
            tokio::net::UnixStream::connect(&socket)
                .await
                .expect("connect"),
        )
        .await
        .expect("second");
    for client in [&first, &second] {
        assert_eq!(read_thread(client).await.is_error, Some(false));
    }
}

#[test]
fn the_shipped_configuration_is_off_and_a_bad_file_is_refused() {
    assert_eq!(McpConfig::shipped().expect("shipped"), McpConfig::default());
    assert_eq!(McpConfig::default().expose, actions_mcp::McpExpose::Off);
    assert!(McpConfig::parse("client = 5").is_err());
    assert!(McpConfig::parse("acess = \"on\"").is_err());
    // The switch is not a key of this file any more.
    assert!(McpConfig::parse("access = \"on\"").is_err());
}

#[test]
fn the_command_line_reads_socket_and_client_and_refuses_the_rest() {
    use actions_mcp::Args;
    let parsed = Args::parse(["--socket".into(), "/run/x.sock".into()]).expect("socket");
    assert_eq!(parsed.socket, Some("/run/x.sock".into()));
    assert!(Args::parse(["--bogus".into()]).is_err());
    assert!(Args::parse(["--client".into()]).is_err());
}

#[test]
fn the_units_command_line_is_one_the_binary_reads() {
    let unit = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../dist/actions-mcp.service"
    ))
    .expect("unit");
    let exec = unit
        .lines()
        .find_map(|l| l.strip_prefix("ExecStart="))
        .expect("ExecStart");
    let args = exec.split_whitespace().skip(1).map(str::to_owned);
    let parsed = actions_mcp::Args::parse(args).expect("the unit's arguments");
    assert!(parsed.socket.is_some(), "the unit serves a socket");
}

#[test]
fn write_schema_installs_the_settings_schema_without_touching_the_bus() {
    let dir = tempfile::tempdir().expect("scratch");
    let out = dir.path().join("schemas");
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_actions-mcp"))
        .env_clear()
        .args(["--write-schema", out.to_str().expect("utf-8")])
        .status()
        .expect("actions-mcp starts");
    assert!(status.success());
    let written = std::fs::read_to_string(out.join("docket.settings.toml")).expect("schema");
    assert_eq!(written, actions_mcp::SCHEMA);
}
