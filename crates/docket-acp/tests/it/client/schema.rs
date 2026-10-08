//! Conformance: every message we send the agent, and every message the fake agent sends us,
//! fits the protocol-v1 JSON Schema kept beside these tests (the same copy the server's tests
//! use, never fetched). The fake agent's messages are checked too, so a green run means the
//! script spoke the protocol and not a dialect.

use super::agent::{AGENT_SESSION, Act, View, call, call_with, say};
use super::rig::{Setup, once, read, run_turn, started, tool, write};
use docket_shell::fake::Script;
use serde_json::{Value, json};

fn conforms(schema: &Value, def: &str, value: &Value) {
    let root = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$ref": format!("#/$defs/{def}"),
        "$defs": schema["$defs"],
    });
    let validator = jsonschema::validator_for(&root).expect("validator");
    let errors: Vec<String> = validator
        .iter_errors(value)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "{def}: {errors:?} in {value}");
}

/// The definition a request's parameters must fit.
fn params_def(method: &str) -> &'static str {
    match method {
        "initialize" => "InitializeRequest",
        "session/new" => "NewSessionRequest",
        "session/prompt" => "PromptRequest",
        "session/cancel" => "CancelNotification",
        "session/update" => "SessionNotification",
        "fs/read_text_file" => "ReadTextFileRequest",
        "fs/write_text_file" => "WriteTextFileRequest",
        "session/request_permission" => "RequestPermissionRequest",
        "terminal/create" => "CreateTerminalRequest",
        "terminal/output" => "TerminalOutputRequest",
        "terminal/wait_for_exit" => "WaitForTerminalExitRequest",
        "terminal/kill" => "KillTerminalRequest",
        "terminal/release" => "ReleaseTerminalRequest",
        "elicitation/create" => "CreateElicitationRequest",
        other => panic!("no definition for {other}"),
    }
}

/// The definition a result must fit.
fn result_def(method: &str) -> &'static str {
    match method {
        "initialize" => "InitializeResponse",
        "session/new" => "NewSessionResponse",
        "session/prompt" => "PromptResponse",
        "fs/read_text_file" => "ReadTextFileResponse",
        "fs/write_text_file" => "WriteTextFileResponse",
        "session/request_permission" => "RequestPermissionResponse",
        "terminal/create" => "CreateTerminalResponse",
        "terminal/output" => "TerminalOutputResponse",
        "terminal/wait_for_exit" => "WaitForTerminalExitResponse",
        "terminal/kill" => "KillTerminalResponse",
        "terminal/release" => "ReleaseTerminalResponse",
        "elicitation/create" => "CreateElicitationResponse",
        other => panic!("no definition for {other}"),
    }
}

/// Checks `lines` (one side's output) against `asked` (the other side's requests, by id).
fn check_side(schema: &Value, lines: &[Value], asked: &[Value]) {
    for line in lines {
        if let Some(method) = line["method"].as_str() {
            conforms(schema, params_def(method), &line["params"]);
        } else if let Some(result) = line.get("result") {
            let request = asked
                .iter()
                .find(|r| r["id"] == line["id"] && r.get("method").is_some())
                .unwrap_or_else(|| panic!("a result for nothing asked: {line}"));
            conforms(
                schema,
                result_def(request["method"].as_str().expect("method")),
                result,
            );
        } else if let Some(error) = line.get("error") {
            conforms(schema, "Error", error);
        }
    }
}

fn script() -> Vec<Act> {
    let id = |r: &super::agent::Replies| r["term"].clone().unwrap()["terminalId"].clone();
    vec![
        call("r", "fs/read_text_file", read("/work/app/a.txt")),
        call("w", "fs/write_text_file", write("/work/app/b.txt", "x")),
        call(
            "p",
            "session/request_permission",
            tool("edit", "e", &["/work/app/c.txt"], json!({})),
        ),
        call(
            "t",
            "terminal/create",
            json!({"sessionId": AGENT_SESSION, "command": "true"}),
        ),
        call(
            "term",
            "terminal/create",
            json!({"sessionId": AGENT_SESSION, "command": "true"}),
        ),
        call_with(
            "o",
            "terminal/output",
            move |r| json!({"sessionId": AGENT_SESSION, "terminalId": id(r)}),
        ),
        call_with(
            "x",
            "terminal/wait_for_exit",
            move |r| json!({"sessionId": AGENT_SESSION, "terminalId": id(r)}),
        ),
        call_with(
            "k",
            "terminal/kill",
            move |r| json!({"sessionId": AGENT_SESSION, "terminalId": id(r)}),
        ),
        call_with(
            "l",
            "terminal/release",
            move |r| json!({"sessionId": AGENT_SESSION, "terminalId": id(r)}),
        ),
        call(
            "e",
            "elicitation/create",
            json!({
            "sessionId": AGENT_SESSION, "mode": "form", "message": "name?",
            "requestedSchema": {"type": "object", "properties": {"n": {"type": "string"}}}}),
        ),
        Act::Update(
            json!({"sessionUpdate": "tool_call", "toolCallId": "z", "title": "t",
                           "kind": "search", "status": "in_progress"}),
        ),
        Act::Update(
            json!({"sessionUpdate": "tool_call_update", "toolCallId": "z", "status": "completed"}),
        ),
        Act::Update(json!({"sessionUpdate": "usage_update", "used": 10, "size": 100})),
        say("ok"),
        Act::Stop("end_turn"),
    ]
}

#[tokio::test]
async fn every_message_both_ways_fits_the_protocol_schema() {
    let schema: Value =
        serde_json::from_str(include_str!("../../schema/schema.json")).expect("schema");
    let (mut rig, files) = started(Setup {
        turns: vec![script()],
        answers: vec![once(), once(), once(), once()],
        scripts: vec![Script::done("", 0), Script::done("", 0)],
        ..Setup::default()
    })
    .await;
    files.put(&super::rig::abs("/work/app/a.txt"), "a");
    run_turn(&mut rig, "go").await;
    let view: &View = &rig.agent;
    let to_agent = view.lines();
    let from_agent = view.sent();
    // The handshake replies come from the agent: its `sent` holds them too.
    check_side(&schema, &to_agent, &from_agent);
    check_side(&schema, &from_agent, &to_agent);
    assert!(to_agent.len() > 12, "the whole conversation was checked");
}
