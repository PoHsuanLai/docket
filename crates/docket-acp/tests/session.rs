//! The server against a fake editor: a whole session, the extra permission gate, refusals,
//! modes, cancel. The host is the scripted `FakeHost`; no network, no process.

mod support;

use docket_core::{DenyCode, StepEnd, TurnSource};
use docket_session::TurnEnd;
use docket_session::fake::MemoryLog;
use prov::Effect;
use serde_json::{Value, json};
use std::sync::Arc;
use support::*;

const EDITOR: &str = "acp.zed";

/// A script of one turn: look, change something (the editor is asked, our gate then refuses
/// it), and say so.
fn one_turn() -> Vec<Vec<Vec<docket_session::BackendEvent>>> {
    vec![vec![vec![
        words("Looking at the invoices."),
        started(1, "mail.search", Effect::Read),
        step(1, "mail.search", Effect::Read, done("3 messages")),
        started(2, "mail.archive", Effect::UndoableWrite),
        step(
            2,
            "mail.archive",
            Effect::UndoableWrite,
            denied(DenyCode::OutsideTask),
        ),
        words("I could not archive them."),
        end(TurnEnd::Done),
    ]]]
}

fn golden(name: &str, got: &str) {
    let path = format!("{}/tests/golden/{name}", env!("CARGO_MANIFEST_DIR"));
    if std::env::var("DOCKET_ACP_BLESS").is_ok() {
        std::fs::write(&path, got).expect("bless");
    }
    let want = std::fs::read_to_string(&path).expect("golden file");
    assert_eq!(got, want, "{name} changed; DOCKET_ACP_BLESS=1 rewrites it");
}

fn kinds(updates: &[Value]) -> Vec<(String, String)> {
    updates
        .iter()
        .map(|u| {
            let status = u["status"].as_str().unwrap_or("").to_owned();
            (u["sessionUpdate"].as_str().unwrap_or("").to_owned(), status)
        })
        .collect()
}

#[tokio::test]
async fn a_whole_session_in_order_and_golden() {
    let log = Arc::new(MemoryLog::new());
    let (mut server, mut ed) = rig(&log, one_turn(), EDITOR);
    let script = async move {
        let init = ed.initialize().await;
        assert_eq!(init["result"]["protocolVersion"], 1);
        assert_eq!(init["result"]["agentCapabilities"]["loadSession"], true);
        let session = ed.new_session("/work/proj").await;
        let reply = ed
            .prompt(&session, "archive the invoices", &mut allow)
            .await;
        assert_eq!(reply["result"]["stopReason"], "end_turn");
        let updates = ed.updates();
        let kinds = kinds(&updates);
        let want = [
            ("agent_message_chunk", ""),
            ("tool_call", ""),
            ("tool_call_update", "in_progress"),
            ("tool_call_update", "completed"),
            ("tool_call", ""),
            ("tool_call_update", "in_progress"),
            ("tool_call_update", "failed"),
            ("agent_message_chunk", ""),
        ];
        let want: Vec<(String, String)> = want
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        assert_eq!(kinds, want);
        ed.transcript()
    };
    let (ran, transcript) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
    golden("session.jsonl", &transcript);
}

#[tokio::test]
async fn the_editor_allowing_a_call_does_not_get_it_past_our_gate_and_the_failure_is_coarse() {
    let log = Arc::new(MemoryLog::new());
    let (mut server, mut ed) = rig(&log, one_turn(), EDITOR);
    let script = async move {
        ed.initialize().await;
        let session = ed.new_session("/work/proj").await;
        ed.prompt(&session, "archive the invoices", &mut allow)
            .await;
        let failed: Vec<Value> = ed
            .updates()
            .into_iter()
            .filter(|u| u["status"] == "failed")
            .collect();
        assert_eq!(failed.len(), 1);
        assert_eq!(
            failed[0]["content"][0]["content"]["text"],
            "Outside what you asked for."
        );
        // The editor was asked once: for the write, not for the read.
        assert_eq!(ed.permission_requests().len(), 1);
        ed.transcript()
    };
    let (_, transcript) = tokio::join!(server.run(), script);
    for banned in [
        "rawOutput",
        "rawInput",
        "allow_always",
        "reject_always",
        "policy",
        "reviewer",
    ] {
        assert!(
            !transcript.contains(banned),
            "{banned} appeared in {transcript}"
        );
    }
}

#[tokio::test]
async fn each_refusal_reaches_the_editor_as_failed_with_a_coarse_sentence() {
    use docket_core::{CallRefusal, ConfirmEnd};
    let ends = [
        denied(DenyCode::NotAllowed),
        denied(DenyCode::NeedsUser),
        denied(DenyCode::Repeated),
        StepEnd::Refused(CallRefusal::Timeout),
        StepEnd::Unconfirmed(ConfirmEnd::Refused),
        StepEnd::Interrupted,
    ];
    let events: Vec<_> = ends
        .into_iter()
        .enumerate()
        .flat_map(|(i, e)| {
            let n = i as u64 + 1;
            [
                started(n, "mail.search", Effect::Read),
                step(n, "mail.search", Effect::Read, e),
            ]
        })
        .chain([end(TurnEnd::Done)])
        .collect();
    let log = Arc::new(MemoryLog::new());
    let (mut server, mut ed) = rig(&log, vec![vec![events]], EDITOR);
    let script = async move {
        ed.initialize().await;
        let session = ed.new_session("/work").await;
        ed.prompt(&session, "go", &mut allow).await;
        let said: Vec<String> = ed
            .updates()
            .iter()
            .filter(|u| u["status"] == "failed")
            .map(|u| {
                u["content"][0]["content"]["text"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(said.len(), 6);
        assert!(
            said.iter().all(|s| s.ends_with('.') && s.len() < 60),
            "{said:?}"
        );
        assert_eq!(
            said.iter().collect::<std::collections::BTreeSet<_>>().len(),
            6
        );
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
}

#[tokio::test]
async fn a_reject_stops_the_turn_and_the_call_never_runs() {
    let log = Arc::new(MemoryLog::new());
    let (mut server, mut ed) = rig(&log, one_turn(), EDITOR);
    let script = async move {
        ed.initialize().await;
        let session = ed.new_session("/work/proj").await;
        let reply = ed.prompt(&session, "archive", &mut reject).await;
        assert_eq!(reply["result"]["stopReason"], "cancelled");
        let last = ed.updates().pop().unwrap();
        assert_eq!(last["status"], "failed");
        assert_eq!(
            last["content"][0]["content"]["text"],
            "Declined in the editor."
        );
        session
    };
    let (_, session) = tokio::join!(server.run(), script);
    assert_eq!(server.into_host().cancelled.len(), 1, "{session}");
}

#[tokio::test]
async fn allow_always_is_never_offered_and_choosing_it_anyway_is_a_reject() {
    let log = Arc::new(MemoryLog::new());
    let (mut server, mut ed) = rig(&log, one_turn(), EDITOR);
    let script = async move {
        ed.initialize().await;
        let session = ed.new_session("/work/proj").await;
        let mut cheat = |_: &Value| {
            Some(json!({"outcome": {"outcome": "selected", "optionId": "allow_always"}}))
        };
        let reply = ed.prompt(&session, "archive", &mut cheat).await;
        assert_eq!(reply["result"]["stopReason"], "cancelled");
        let asked = ed.permission_requests();
        let options = asked[0]["params"]["options"].as_array().unwrap();
        let kinds: Vec<&str> = options
            .iter()
            .map(|o| o["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["allow_once", "reject_once"]);
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
}

#[tokio::test]
async fn a_cancel_while_the_editor_is_asked_ends_the_turn_cancelled() {
    let log = Arc::new(MemoryLog::new());
    let (mut server, mut ed) = rig(&log, one_turn(), EDITOR);
    let script = async move {
        ed.initialize().await;
        let session = ed.new_session("/work/proj").await;
        let params = json!({"sessionId": session, "prompt": [{"type": "text", "text": "archive"}]});
        let id = ed.send("session/prompt", params, "PromptRequest").await;
        // Read to the permission request, then cancel instead of answering.
        loop {
            if ed.recv().await["method"] == "session/request_permission" {
                break;
            }
        }
        ed.notify(
            "session/cancel",
            json!({"sessionId": session}),
            "CancelNotification",
        )
        .await;
        let reply = ed.until_reply(id, "PromptResponse", &mut |_| None).await;
        assert_eq!(reply["result"]["stopReason"], "cancelled");
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
    // (the host heard the cancel)
}

#[tokio::test]
async fn read_only_mode_stops_a_write_without_asking() {
    let log = Arc::new(MemoryLog::new());
    let (mut server, mut ed) = rig(&log, one_turn(), EDITOR);
    let script = async move {
        ed.initialize().await;
        let session = ed.new_session("/work/proj").await;
        let reply = ed
            .call(
                "session/set_mode",
                json!({"sessionId": session, "modeId": "read-only"}),
                "SetSessionModeRequest",
                "SetSessionModeResponse",
            )
            .await;
        assert!(reply.get("result").is_some());
        let done = ed.prompt(&session, "archive", &mut allow).await;
        assert_eq!(done["result"]["stopReason"], "cancelled");
        assert!(ed.permission_requests().is_empty());
        let last = ed.updates().pop().unwrap();
        assert_eq!(
            last["content"][0]["content"]["text"],
            "Not allowed in read-only mode."
        );
        let bad = ed
            .call(
                "session/set_mode",
                json!({"sessionId": session, "modeId": "yolo"}),
                "SetSessionModeRequest",
                "SetSessionModeResponse",
            )
            .await;
        assert_eq!(bad["error"]["code"], -32602);
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
}

#[tokio::test]
async fn the_prompt_text_is_the_turn_and_attachments_are_not() {
    let log = Arc::new(MemoryLog::new());
    let (mut server, mut ed) = rig(&log, vec![vec![vec![end(TurnEnd::Done)]]], EDITOR);
    let script = async move {
        ed.initialize().await;
        let session = ed.new_session("/work/proj").await;
        let params = json!({"sessionId": session, "prompt": [
            {"type": "text", "text": "summarise this"},
            {"type": "resource_link", "name": "notes", "uri": "file:///work/proj/notes.md"},
            {"type": "text", "text": "briefly"},
        ]});
        let id = ed.send("session/prompt", params, "PromptRequest").await;
        let reply = ed.until_reply(id, "PromptResponse", &mut |_| None).await;
        assert_eq!(reply["result"]["stopReason"], "end_turn");
        let none = json!({"sessionId": session, "prompt": [{"type": "resource_link", "name": "n", "uri": "file:///x"}]});
        let id = ed.send("session/prompt", none, "PromptRequest").await;
        let reply = ed.until_reply(id, "PromptResponse", &mut |_| None).await;
        assert_eq!(reply["error"]["code"], -32602);
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
    let host = server.into_host();
    assert_eq!(host.turns.len(), 1);
    let turn = &host.turns[0].1;
    assert_eq!(turn.text, "summarise this\nbriefly");
    assert_eq!(turn.from, TurnSource::Editor(app(EDITOR)));
}

#[tokio::test]
async fn editor_mcp_servers_are_ignored_and_unknown_methods_refused() {
    let log = Arc::new(MemoryLog::new());
    let (mut server, mut ed) = rig(&log, vec![vec![]], EDITOR);
    let script = async move {
        // Before `initialize` nothing else is served.
        let early = ed
            .call(
                "session/new",
                json!({"cwd": "/w", "mcpServers": []}),
                "NewSessionRequest",
                "NewSessionResponse",
            )
            .await;
        assert_eq!(early["error"]["code"], -32600);
        ed.initialize().await;
        let mcp = json!([{"name": "evil", "command": "/bin/sh", "args": ["-c", "x"], "env": []}]);
        let made = ed
            .call(
                "session/new",
                json!({"cwd": "/w", "mcpServers": mcp}),
                "NewSessionRequest",
                "NewSessionResponse",
            )
            .await;
        assert!(made["result"]["sessionId"].is_string());
        for method in [
            "fs/read_text_file",
            "terminal/create",
            "session/resume",
            "authenticate",
            "session/cancel",
        ] {
            let id = ed.send_unchecked(method).await;
            let reply = ed
                .until_reply(id, "InitializeResponse", &mut |_| None)
                .await;
            assert_eq!(reply["error"]["code"], -32601, "{method}");
        }
        ed.raw("not json").await;
        assert_eq!(ed.recv().await["error"]["code"], -32700);
        let relative = ed
            .call(
                "session/new",
                json!({"cwd": "work", "mcpServers": []}),
                "NewSessionRequest",
                "NewSessionResponse",
            )
            .await;
        assert_eq!(relative["error"]["code"], -32602);
    };
    let (ran, ()) = tokio::join!(server.run(), script);
    assert!(ran.is_ok());
}
