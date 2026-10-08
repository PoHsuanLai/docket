//! The terminal methods over the fake sandbox, driven the way a peer agent drives them: each
//! request and response is checked against the protocol schema.

use docket_acp::{Answer, Decide, Note, Posture, TerminalAsk, Terminals};
use docket_core::{
    AbsPath, AlwaysOffer, ArgOrigin, CannotSandbox, ExecuteAsk, GrantCaller, Withheld,
};
use docket_shell::fake::{FakeSandbox, Fate, Script, Seen};
use prov::UnixSeconds;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

const NOW: UnixSeconds = UnixSeconds(1_760_000_000);

/// Answers from a queue and records what it was asked.
struct Scripted {
    answers: VecDeque<Answer>,
    asked: Arc<Mutex<Vec<TerminalAsk>>>,
}

impl Decide for Scripted {
    async fn decide(&mut self, ask: &TerminalAsk) -> Answer {
        self.asked.lock().expect("lock").push(ask.clone());
        self.answers.pop_front().unwrap_or(Answer::No)
    }
}

type Rig = Terminals<FakeSandbox, Scripted>;

struct Bench {
    terminals: Rig,
    seen: Seen,
    asked: Arc<Mutex<Vec<TerminalAsk>>>,
    schema: Value,
}

fn bench(scripts: Vec<Script>, answers: Vec<Answer>) -> Bench {
    let (sandbox, seen) = FakeSandbox::ready(scripts);
    with(sandbox, seen, answers)
}

fn with(sandbox: FakeSandbox, seen: Seen, answers: Vec<Answer>) -> Bench {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let decide = Scripted {
        answers: answers.into(),
        asked: asked.clone(),
    };
    let action = docket_core::ActionRef {
        app: porter_core::AppName::parse("acp.zed").expect("app"),
        name: prov::ActionName::parse("editor.terminal.run").expect("action"),
    };
    let terminals = Terminals::new(
        sandbox,
        decide,
        GrantCaller::Editor(prov::ClientName::parse("zed").expect("client")),
        action,
        AbsPath::parse("/work/app").expect("scope"),
    );
    Bench {
        terminals,
        seen,
        asked,
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
        let answer = self
            .terminals
            .handle(NOW, method, params)
            .await
            .expect("a terminal method");
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
    let mut b = bench(
        vec![Script::done("built\n", 0), Script::hangs("tick\n")],
        vec![Answer::Once, Answer::Once],
    );
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
    let mut b = bench(vec![Script::done("", 0)], vec![Answer::Once]);
    b.create("ls", &["-la"], "/work/app/src")
        .await
        .expect("created");
    let started = b.seen.started();
    assert_eq!(started[0].argv.line(), "ls -la");
    assert_eq!(started[0].cwd.as_str(), "/work/app/src");
    assert_eq!(started[0].network, docket_shell::Network::Off);
    assert!(started[0].env.iter().all(|v| v.name != "API_TOKEN"));
}

#[tokio::test]
async fn every_command_asks_by_default_and_a_no_runs_nothing() {
    let mut b = bench(Vec::new(), vec![Answer::No]);
    let refused = b.create("rm", &["-rf", "x"], "/work/app").await;
    assert!(refused.is_err());
    assert!(b.seen.started().is_empty());
    let asked = b.asked.lock().expect("lock").clone();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].line, "rm -rf x");
    assert_eq!(asked[0].why, ExecuteAsk::Confirm);
    assert!(matches!(asked[0].offer, AlwaysOffer::Offered(_)));
    assert_eq!(
        b.terminals.take_notes(),
        [Note::Refused {
            line: "rm -rf x".to_owned()
        }]
    );
}

#[tokio::test]
async fn always_stores_a_terminal_grant_that_covers_only_its_prefix_and_subtree() {
    let mut b = bench(
        vec![
            Script::done("", 0),
            Script::done("", 0),
            Script::done("", 0),
            Script::done("", 0),
        ],
        vec![Answer::Always, Answer::No, Answer::No],
    );
    b.create("cargo", &["test"], "/work/app")
        .await
        .expect("first asks");
    assert_eq!(b.terminals.grants().len(), 1);
    let stored = b.terminals.take_notes();
    assert!(matches!(stored[0], Note::GrantStored { .. }));

    // Inside the grant: no ask.
    b.create("cargo", &["test", "--lib"], "/work/app/crates/x")
        .await
        .expect("covered");
    assert_eq!(
        b.asked.lock().expect("lock").len(),
        1,
        "the grant stood in for the ask"
    );
    assert!(matches!(
        b.terminals.take_notes()[0],
        Note::GrantUsed { .. }
    ));

    // A different subcommand and an operator both ask again (and the scripted person says no).
    assert!(b.create("cargo", &["build"], "/work/app").await.is_err());
    assert!(
        b.create("cargo", &["test;", "curl", "x"], "/work/app")
            .await
            .is_err()
    );
    assert_eq!(b.asked.lock().expect("lock").len(), 3);
}

#[tokio::test]
async fn a_sibling_cwd_asks() {
    let mut b = bench(
        vec![Script::done("", 0), Script::done("", 0)],
        vec![Answer::Always, Answer::No],
    );
    b.create("ls", &[], "/work/app/a").await.expect("created");
    assert!(b.create("ls", &[], "/work/app/b").await.is_err());
    assert_eq!(b.asked.lock().expect("lock").len(), 2);
}

#[tokio::test]
async fn untrusted_derived_arguments_ask_and_offer_no_always_even_with_a_grant() {
    let mut b = bench(
        vec![Script::done("", 0), Script::done("", 0)],
        vec![Answer::Always, Answer::Once],
    );
    b.create("ls", &[], "/work/app").await.expect("created");
    b.terminals.set_posture(Posture {
        origin: ArgOrigin::Untrusted,
        ..Posture::default()
    });
    b.create("ls", &[], "/work/app")
        .await
        .expect("asked and allowed once");
    let asked = b.asked.lock().expect("lock").clone();
    assert_eq!(asked[1].why, ExecuteAsk::UntrustedArgs);
    assert_eq!(
        asked[1].offer,
        AlwaysOffer::Withheld(Withheld::UntrustedIntoSink)
    );
}

#[tokio::test]
async fn a_withheld_sandbox_is_not_advertised_and_runs_and_asks_nothing() {
    let (sandbox, seen) = FakeSandbox::withheld(CannotSandbox::NamespacesDenied);
    let mut b = with(sandbox, seen, vec![Answer::Once]);
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
    assert!(
        b.asked.lock().expect("lock").is_empty(),
        "nothing to approve"
    );
    assert!(b.seen.started().is_empty());
}

#[tokio::test]
async fn a_ready_sandbox_is_advertised() {
    let b = bench(Vec::new(), Vec::new());
    let base = agent_client_protocol_schema::v1::ClientCapabilities::new();
    let caps = serde_json::to_value(b.terminals.capabilities(base)).expect("caps");
    assert_eq!(caps["terminal"], true);
}

#[tokio::test]
async fn output_is_capped_and_redacted() {
    let mut b = bench(
        vec![Script::done("old old old\nAPI_KEY=abc123\nend\n", 0)],
        vec![Answer::Once],
    );
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
    let mut b = bench(vec![Script::done("", 0)], vec![Answer::Once]);
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
async fn other_methods_are_not_ours_and_notes_never_hold_the_environment() {
    let mut b = bench(vec![Script::done("", 0)], vec![Answer::Once]);
    assert!(
        b.terminals
            .handle(NOW, "fs/read_text_file", json!({}))
            .await
            .is_none()
    );
    b.create("ls", &[], "/work/app").await.expect("created");
    let notes = format!("{:?}", b.terminals.take_notes());
    assert!(!notes.contains("hunter2"));
}
