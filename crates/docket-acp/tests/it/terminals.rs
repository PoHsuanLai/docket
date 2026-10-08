//! The terminal methods over the fake sandbox, driven the way a peer agent drives them: each
//! request and response is checked against the protocol schema.

use bulkhead::CannotSandbox;
use bulkhead::fake::{FakeSandbox, Fate, Script, Seen};
use docket_acp::Terminals;
use docket_core::AbsPath;
use serde_json::{Value, json};

/// The scope the host was opened in.
const SCOPE: &str = "/work/app";

struct Bench {
    terminals: Terminals<FakeSandbox>,
    seen: Seen,
    schema: Value,
}

fn bench(scripts: Vec<Script>) -> Bench {
    let (sandbox, seen) = FakeSandbox::ready(scripts);
    with(sandbox, seen)
}

fn with(sandbox: FakeSandbox, seen: Seen) -> Bench {
    Bench {
        terminals: Terminals::new(sandbox),
        seen,
        schema: serde_json::from_str(include_str!("../schema/schema.json")).expect("schema"),
    }
}

impl Bench {
    fn conforms(&self, def: &str, value: &Value) {
        let root = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$ref": format!("#/$defs/{def}"),
            "$defs": self.schema["$defs"],
        });
        let validator = jsonschema::validator_for(&root).expect("validator");
        let errors: Vec<String> = validator
            .iter_errors(value)
            .map(|e| e.to_string())
            .collect();
        assert!(errors.is_empty(), "{def}: {errors:?} in {value}");
    }

    async fn call(
        &mut self,
        method: &str,
        params: Value,
        request: &str,
        response: &str,
    ) -> Result<Value, Value> {
        self.conforms(request, &params);
        let answer = if method == "terminal/create" {
            self.terminals
                .create(&AbsPath::parse(SCOPE).expect("scope"), params)
        } else {
            self.terminals
                .handle(method, params)
                .expect("a terminal method")
        };
        match answer {
            Ok(value) => {
                self.conforms(response, &value);
                Ok(value)
            }
            Err(error) => Err(serde_json::to_value(error).expect("error")),
        }
    }

    async fn create(&mut self, command: &str, args: &[&str], cwd: &str) -> Result<String, Value> {
        let params = json!({
            "sessionId": "s1", "command": command, "args": args, "cwd": cwd,
            "env": [{"name": "API_TOKEN", "value": "hunter2"}, {"name": "LANG", "value": "C"}],
        });
        let made = self
            .call(
                "terminal/create",
                params,
                "CreateTerminalRequest",
                "CreateTerminalResponse",
            )
            .await?;
        Ok(made["terminalId"].as_str().expect("id").to_owned())
    }

    async fn on(
        &mut self,
        method: &str,
        id: &str,
        request: &str,
        response: &str,
    ) -> Result<Value, Value> {
        let params = json!({"sessionId": "s1", "terminalId": id});
        self.call(method, params, request, response).await
    }

    async fn output(&mut self, id: &str) -> Value {
        self.on(
            "terminal/output",
            id,
            "TerminalOutputRequest",
            "TerminalOutputResponse",
        )
        .await
        .expect("output")
    }
}

#[tokio::test]
async fn create_output_wait_kill_release_through_the_sandbox() {
    let mut b = bench(vec![Script::done("built\n", 0), Script::hangs("tick\n")]);
    let quick = b
        .create("cargo", &["build"], "/work/app")
        .await
        .expect("created");
    let out = b.output(&quick).await;
    assert_eq!(out["output"], "built\n");
    assert_eq!(out["truncated"], false);
    assert_eq!(out["exitStatus"]["exitCode"], 0);
    let waited = b
        .on(
            "terminal/wait_for_exit",
            &quick,
            "WaitForTerminalExitRequest",
            "WaitForTerminalExitResponse",
        )
        .await
        .expect("wait");
    assert_eq!(waited["exitCode"], 0);

    let slow = b
        .create("sleep", &["99"], "/work/app")
        .await
        .expect("created");
    assert!(
        b.output(&slow).await["exitStatus"].is_null(),
        "still running"
    );
    b.on(
        "terminal/kill",
        &slow,
        "KillTerminalRequest",
        "KillTerminalResponse",
    )
    .await
    .expect("kill");
    let after = b.output(&slow).await;
    assert_eq!(
        after["output"], "tick\n",
        "the terminal stays valid after kill"
    );
    assert_eq!(after["exitStatus"]["signal"], "KILL");
    b.on(
        "terminal/release",
        &slow,
        "ReleaseTerminalRequest",
        "ReleaseTerminalResponse",
    )
    .await
    .expect("release");
    assert_eq!(b.seen.fates(), [Fate::Running, Fate::Killed]);
    assert!(
        b.on(
            "terminal/output",
            &slow,
            "TerminalOutputRequest",
            "TerminalOutputResponse"
        )
        .await
        .is_err(),
        "a released terminal is gone"
    );
}

#[tokio::test]
async fn the_command_runs_with_the_cwd_a_cleared_env_and_no_network() {
    let mut b = bench(vec![Script::done("", 0)]);
    b.create("ls", &["-la"], "/work/app/src")
        .await
        .expect("created");
    let started = b.seen.started();
    assert_eq!(started[0].argv.line(), "ls -la");
    assert_eq!(started[0].cwd.as_str(), "/work/app/src");
    assert_eq!(started[0].network, bulkhead::Network::Off);
    assert!(started[0].env.iter().all(|v| v.name != "API_TOKEN"));
}

#[tokio::test]
async fn a_withheld_sandbox_is_not_advertised_and_runs_nothing() {
    let (sandbox, seen) = FakeSandbox::withheld(CannotSandbox::NamespacesDenied);
    let mut b = with(sandbox, seen);
    let base = agent_client_protocol_schema::v1::ClientCapabilities::new();
    let caps = serde_json::to_value(b.terminals.capabilities(base)).expect("caps");
    assert_eq!(caps["terminal"], false);
    let refused = b.create("ls", &[], "/work/app").await.expect_err("refused");
    assert!(
        refused["message"]
            .as_str()
            .expect("message")
            .contains("cannot sandbox")
    );
    assert!(b.seen.started().is_empty());
}

#[tokio::test]
async fn a_ready_sandbox_is_advertised() {
    let b = bench(Vec::new());
    let base = agent_client_protocol_schema::v1::ClientCapabilities::new();
    let caps = serde_json::to_value(b.terminals.capabilities(base)).expect("caps");
    assert_eq!(caps["terminal"], true);
}

#[tokio::test]
async fn output_is_capped_and_redacted() {
    let mut b = bench(vec![Script::done("old old old\nAPI_KEY=abc123\nend\n", 0)]);
    let params =
        json!({"sessionId": "s1", "command": "env", "cwd": "/work/app", "outputByteLimit": 30});
    let made = b
        .call(
            "terminal/create",
            params,
            "CreateTerminalRequest",
            "CreateTerminalResponse",
        )
        .await
        .expect("created");
    let out = b.output(made["terminalId"].as_str().expect("id")).await;
    let text = out["output"].as_str().expect("text");
    assert!(text.len() <= 30, "{text:?}");
    assert!(!text.contains("abc123"), "{text:?}");
    assert!(text.contains("API_KEY=[redacted]"), "{text:?}");
    assert_eq!(out["truncated"], true);
}

#[tokio::test]
async fn a_cwd_outside_the_session_and_a_foreign_session_are_refused() {
    let mut b = bench(vec![Script::done("", 0)]);
    assert!(b.create("ls", &[], "/etc").await.is_err());
    assert!(b.create("ls", &[], "/work/app/../other").await.is_err());
    let id = b.create("ls", &[], "/work/app").await.expect("created");
    let params = json!({"sessionId": "someone-else", "terminalId": id});
    let foreign = b
        .call(
            "terminal/output",
            params,
            "TerminalOutputRequest",
            "TerminalOutputResponse",
        )
        .await;
    assert!(
        foreign.is_err(),
        "another session cannot read this terminal"
    );
}

#[tokio::test]
async fn other_methods_are_not_ours() {
    let mut b = bench(vec![Script::done("", 0)]);
    assert!(b.terminals.handle("fs/read_text_file", json!({})).is_none());
    assert!(
        b.terminals.handle("terminal/create", json!({})).is_none(),
        "create needs the session's scope, and the router's yes: it is not a plain method"
    );
}
