//! The router's sheets for an editor's session go to the editor, over the bus: intentd hands them
//! to the `docket-acp` process (which serves `Confirm1` under its own name), and the editor sees
//! them as ACP permission requests. The desktop's sheet (sill) is never asked for these.

use crate::acp::{editor_of, permissions, prompt, session, sheet_titles, titles};
use crate::support::*;
use docket_accept::world::{Consent, World};
use serde_json::{Value, json};

fn selected(option: &str) -> Value {
    json!({"outcome": {"outcome": "selected", "optionId": option}})
}

/// Whether `request` is the router's sheet (its tool call is named `sheet-<id>`) and not the
/// editor's own gate at the call's start.
fn is_sheet(request: &Value) -> bool {
    request["params"]["toolCall"]["toolCallId"]
        .as_str()
        .is_some_and(|id| id.starts_with("sheet-"))
}

/// Answers the editor's own gate with `gate` and the router's sheet with `sheet`, and keeps every
/// option id the sheet offered.
fn answers<'a>(
    gate: &'a str,
    sheet: &'a str,
    offered: &'a mut Vec<Vec<String>>,
) -> impl FnMut(&Value) -> Option<Value> + 'a {
    move |request| {
        if !is_sheet(request) {
            return Some(selected(gate));
        }
        let ids = request["params"]["options"]
            .as_array()
            .map(|options| {
                options
                    .iter()
                    .filter_map(|o| o["optionId"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        offered.push(ids);
        Some(selected(sheet))
    }
}

const FORWARD: &str = "forward the Lisbon receipts to accounting";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_editor_sees_the_sheet_as_a_permission_request_and_its_allow_once_performs_the_call() {
    let world = World::start_acp(&binaries(), Consent::Standing, FLOW_A).await;
    let mut editor = editor_of(&world).await;
    let id = session(&mut editor).await;
    let mut offered = Vec::new();
    let reply = editor
        .request(
            "session/prompt",
            prompt(&id, FORWARD),
            &mut answers("allow_once", "allow_once", &mut offered),
        )
        .await;
    assert_eq!(reply["result"]["stopReason"], "end_turn", "{reply}");
    assert_eq!(offered.len(), 1, "{offered:?}");
    assert!(offered[0].contains(&"allow_once".to_owned()), "{offered:?}");
    assert_eq!(permissions(&editor), 2);
    assert!(world.sheet.shown().is_empty(), "sill was not asked");
    assert_eq!(world.mail.messages().len(), 1);
    assert!(titles(&editor.updates()).contains(&"mail.message.forward".to_owned()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_editors_reject_on_the_sheet_refuses_the_call() {
    let world = World::start_acp(&binaries(), Consent::Standing, FLOW_A).await;
    let mut editor = editor_of(&world).await;
    let id = session(&mut editor).await;
    let mut offered = Vec::new();
    let reply = editor
        .request(
            "session/prompt",
            prompt(&id, FORWARD),
            &mut answers("allow_once", "reject_once", &mut offered),
        )
        .await;
    assert!(reply.get("error").is_none(), "{reply}");
    assert_eq!(offered.len(), 1, "{offered:?}");
    let performed = world.mail.performed();
    assert!(
        !performed.iter().any(|a| a.contains("forward")),
        "{performed:?}"
    );
    assert!(world.mail.messages().is_empty());
    assert!(world.sheet.shown().is_empty(), "sill was not asked");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sheet_asked_while_the_router_records_a_turn_reaches_the_editor_and_the_turn_goes_on() {
    let world = World::start_acp(&binaries(), Consent::Standing, ACP_WIDENS).await;
    let mut editor = editor_of(&world).await;
    let id = session(&mut editor).await;
    let mut offered = Vec::new();
    let one = editor
        .request(
            "session/prompt",
            prompt(&id, "find the Lisbon threads"),
            &mut answers("allow_once", "allow_once", &mut offered),
        )
        .await;
    assert_eq!(one["result"]["stopReason"], "end_turn", "{one}");
    assert_eq!(sheet_titles(&editor), Vec::<String>::new());
    // The second turn widens the task: the router asks inside `Session.Turn`, before any call.
    let two = editor
        .request(
            "session/prompt",
            prompt(&id, "and look the contacts up too"),
            &mut answers("allow_once", "allow_once", &mut offered),
        )
        .await;
    assert_eq!(two["result"]["stopReason"], "end_turn", "{two}");
    assert_eq!(sheet_titles(&editor), ["Allow more for this task"]);
    assert!(world.sheet.shown().is_empty(), "sill was not asked");
}
