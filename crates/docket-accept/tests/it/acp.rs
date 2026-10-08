//! An editor drives the companion over ACP: `docket-acp` as a process on the private bus beside
//! the real intentd, memoryd, inferd (replaying a cassette) and the mail provider, spoken to by a
//! scripted editor over stdin and stdout.

use crate::support::*;
use docket_accept::acp::AcpEditor;
use docket_accept::confirm::Verdict;
use docket_accept::world::{Consent, World};
use serde_json::{Value, json};

pub(crate) const CWD: &str = "/work/project";

pub(crate) fn permission(option: &'static str) -> impl FnMut(&Value) -> Option<Value> {
    move |_| Some(json!({"outcome": {"outcome": "selected", "optionId": option}}))
}

pub(crate) async fn session(editor: &mut AcpEditor) -> String {
    let init = json!({"protocolVersion": 1, "clientCapabilities": {}, "clientInfo": {"name": "zed", "version": "1"}});
    let reply = editor.request("initialize", init, &mut |_| None).await;
    assert_eq!(reply["result"]["protocolVersion"], 1, "{reply}");
    let reply = editor
        .request(
            "session/new",
            json!({"cwd": CWD, "mcpServers": []}),
            &mut |_| None,
        )
        .await;
    reply["result"]["sessionId"]
        .as_str()
        .unwrap_or_else(|| panic!("{reply}"))
        .to_owned()
}

pub(crate) fn prompt(id: &str, text: &str) -> Value {
    json!({"sessionId": id, "prompt": [{"type": "text", "text": text}]})
}

const BIN: &str = env!("CARGO_BIN_EXE_accept-acp");

pub(crate) async fn editor_of(world: &World) -> AcpEditor {
    AcpEditor::start(world, std::path::Path::new(BIN)).await
}

pub(crate) fn titles(updates: &[Value]) -> Vec<String> {
    updates
        .iter()
        .filter(|u| u["sessionUpdate"] == "tool_call")
        .map(|u| u["title"].as_str().unwrap_or_default().to_owned())
        .collect()
}

pub(crate) fn permissions(editor: &AcpEditor) -> usize {
    editor
        .seen
        .iter()
        .filter(|m| m["method"] == "session/request_permission")
        .count()
}

pub(crate) fn words(updates: &[Value]) -> Vec<String> {
    updates
        .iter()
        .filter(|u| u["sessionUpdate"] == "agent_message_chunk")
        .map(|u| u["content"]["text"].as_str().unwrap_or_default().to_owned())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_session_initialize_new_prompt_a_permission_round_and_done() {
    let world = World::start_acp(&binaries(), Consent::Standing, FLOW_A).await;
    world.sheet.will(Verdict::Allow);
    let mut editor = editor_of(&world).await;
    let id = session(&mut editor).await;
    let reply = editor
        .request(
            "session/prompt",
            prompt(&id, "forward the Lisbon receipts to accounting"),
            &mut permission("allow_once"),
        )
        .await;
    assert_eq!(reply["result"]["stopReason"], "end_turn", "{reply}");
    let updates = editor.updates();
    assert_eq!(
        titles(&updates),
        [
            "mail.thread.search",
            "mail.contact.search",
            "mail.message.forward"
        ]
    );
    // Reads go on; the forward is the one call the editor was asked about, twice: the editor's
    // own gate at the call's start, then the router's sheet, which no longer goes to the desktop.
    assert_eq!(permissions(&editor), 2);
    assert!(words(&updates).contains(&"Forwarded the Lisbon receipts to Accounting.".to_owned()));
    // The router's own gate still ran: its sheet went to the editor, not the desktop, and the app
    // did the work.
    assert!(world.sheet.shown().is_empty());
    let sent = world.mail.messages();
    assert_eq!(sent.len(), 1, "{sent:#?}");
    assert!(matches!(sent[0].actor, prov::Actor::Companion { .. }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_editors_reject_means_the_app_never_runs_the_call() {
    let world = World::start_acp(&binaries(), Consent::Standing, FLOW_A).await;
    world.sheet.will(Verdict::Allow);
    let mut editor = editor_of(&world).await;
    let id = session(&mut editor).await;
    let reply = editor
        .request(
            "session/prompt",
            prompt(&id, "forward the Lisbon receipts to accounting"),
            &mut permission("reject_once"),
        )
        .await;
    assert_eq!(reply["result"]["stopReason"], "cancelled", "{reply}");
    assert_eq!(permissions(&editor), 1);
    let updates = editor.updates();
    let failed = updates
        .iter()
        .filter(|u| u["sessionUpdate"] == "tool_call_update" && u["status"] == "failed")
        .count();
    assert_eq!(failed, 1, "{updates:#?}");
    // The fake app's call log: the two reads, and no forward. No sheet, no message.
    let performed = world.mail.performed();
    assert!(
        !performed.iter().any(|a| a.contains("forward")),
        "{performed:?}"
    );
    assert!(world.mail.messages().is_empty());
    assert!(world.sheet.shown().is_empty());
}

/// Turn one, then (when `restart`) the host goes and a new one starts, loads the session and
/// goes on with turn two. Returns what turn two showed and what the app did. The sheets the editor was shown are returned too: none, since both turns only read (a later clock second used to make the second turn's policy count as wider, and the router asked "Allow more for this task" in about one run in eight).
async fn two_turns(restart: bool) -> (Vec<Value>, Vec<String>, Vec<String>) {
    let world = World::start_acp(&binaries(), Consent::Standing, ACP_TWO_TURNS).await;
    world.sheet.will(Verdict::Allow);
    let mut editor = editor_of(&world).await;
    let id = session(&mut editor).await;
    let one = editor
        .request(
            "session/prompt",
            prompt(&id, "find the Lisbon threads"),
            &mut permission("allow_once"),
        )
        .await;
    assert_eq!(one["result"]["stopReason"], "end_turn", "{one}");
    let mut editor = if restart {
        drop(editor);
        let mut fresh = editor_of(&world).await;
        let init = json!({"protocolVersion": 1, "clientCapabilities": {}});
        fresh.request("initialize", init, &mut |_| None).await;
        let loaded = fresh
            .request(
                "session/load",
                json!({"sessionId": id, "cwd": CWD, "mcpServers": []}),
                &mut |_| None,
            )
            .await;
        assert!(loaded.get("error").is_none(), "{loaded}");
        // What was stored is replayed: the first turn's call and words.
        assert_eq!(titles(&fresh.updates()), ["mail.thread.search"]);
        fresh
    } else {
        editor
    };
    let before = editor.updates().len();
    let two = editor
        .request(
            "session/prompt",
            prompt(&id, "anything else?"),
            &mut permission("allow_once"),
        )
        .await;
    assert_eq!(two["result"]["stopReason"], "end_turn", "{two}");
    let after = editor.updates().split_off(before);
    assert!(world.sheet.shown().is_empty(), "the desktop was not asked");
    (after, world.mail.performed(), sheet_titles(&editor))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_restart_of_the_host_between_turns_changes_nothing_the_editor_or_the_app_sees() {
    let (steady, steady_performed, steady_sheets) = two_turns(false).await;
    let (restarted, restarted_performed, restarted_sheets) = two_turns(true).await;
    assert_eq!(
        words(&restarted),
        ["Still here, and nothing else was done."]
    );
    assert_eq!(restarted, steady);
    assert_eq!(restarted_performed, steady_performed);
    assert_eq!(restarted_sheets, steady_sheets);
    assert_eq!(steady_sheets, Vec::<String>::new());
}

pub(crate) fn sheet_titles(editor: &AcpEditor) -> Vec<String> {
    editor
        .seen
        .iter()
        .filter(|m| m["method"] == "session/request_permission")
        .filter(|m| {
            m["params"]["toolCall"]["toolCallId"]
                .as_str()
                .is_some_and(|id| id.starts_with("sheet-"))
        })
        .map(|m| {
            m["params"]["toolCall"]["title"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}
