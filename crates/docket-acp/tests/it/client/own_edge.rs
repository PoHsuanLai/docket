//! A permission request for a call to our own tool edge is answered "once" at the door, with no
//! sheet and no ruling of its own: the call is ruled when it reaches the edge. Anything that is
//! not exactly that still goes through the router.

use super::agent::{Act, bridge_call, call};
use super::calls::{agy_options, mcp_request, selected};
use super::edge::{thread, with_mail};
use super::rig::{Rig, no, run_turn};
use docket_session::{BackendEvent, CallEvent};
use serde_json::{Value, json};

const ASK: &str = "session/request_permission";
const READ: &str = "mail__mail_thread_read";

fn ours(tool: &str) -> Option<Value> {
    Some(json!({"tool": tool, "server": "quire"}))
}

/// One turn: the agent asks `request`, then ends. The person refuses any sheet.
async fn ask(request: Value, answers: usize) -> (Rig<super::rig::Fakes>, Vec<BackendEvent>) {
    let (mut rig, dir) = with_mail(
        vec![vec![call("perm", ASK, request), Act::Stop("end_turn")]],
        vec![no(); answers],
    )
    .await;
    std::mem::forget(dir);
    let events = run_turn(&mut rig, "go").await;
    (rig, events)
}

fn once_option(rig: &Rig<super::rig::Fakes>) -> Option<String> {
    selected(&rig.agent.reply("perm"))
}

#[tokio::test]
async fn a_request_for_our_own_tool_is_answered_once_with_no_sheet_and_no_ruling() {
    let request = mcp_request("other", ours(READ), json!({"arguments": {}}), agy_options());
    let (rig, events) = ask(request, 0).await;
    assert_eq!(once_option(&rig).as_deref(), Some("a-once"));
    assert!(rig.sheets().is_empty());
    let recorded = events.iter().any(|e| {
        matches!(e, BackendEvent::Call(CallEvent::Ended(l))
            if l.action.name.as_str() == "acpagent.reported.other")
    });
    assert!(recorded, "{events:?}");
}

#[tokio::test]
async fn a_fetch_kind_for_our_tool_is_answered_the_same_way() {
    let request = mcp_request("fetch", ours(READ), json!({}), agy_options());
    let (rig, _) = ask(request, 0).await;
    assert_eq!(once_option(&rig).as_deref(), Some("a-once"));
    assert!(rig.sheets().is_empty());
}

#[tokio::test]
async fn always_is_never_chosen_even_when_it_is_the_only_way_to_allow() {
    let only_always = json!([
        {"optionId": "a-always", "name": "Always", "kind": "allow_always"},
        {"optionId": "r-once", "name": "No", "kind": "reject_once"}
    ]);
    let request = mcp_request("other", ours(READ), json!({}), only_always);
    let (rig, _) = ask(request, 1).await;
    assert_ne!(once_option(&rig).as_deref(), Some("a-always"));
    assert_eq!(rig.sheets().len(), 1, "no once option: the usual path");
}

#[tokio::test]
async fn a_tool_the_edge_did_not_offer_still_gets_a_sheet() {
    // Hidden from the edge's list, so not offered.
    for tool in ["mail__mail_thread_open", "not_a_tool", ""] {
        let request = mcp_request("other", ours(tool), json!({}), agy_options());
        let (rig, _) = ask(request, 1).await;
        assert_eq!(rig.sheets().len(), 1, "{tool:?}");
        assert_eq!(once_option(&rig).as_deref(), Some("r-once"), "{tool:?}");
    }
}

#[tokio::test]
async fn another_server_a_missing_meta_or_another_kind_still_gets_a_sheet() {
    let foreign = json!({"tool": READ, "server": "elsewhere"});
    let cases = [
        mcp_request("other", Some(foreign), json!({}), agy_options()),
        mcp_request("other", None, json!({"arguments": {}}), agy_options()),
        mcp_request("execute", ours(READ), json!({}), agy_options()),
        mcp_request("edit", ours(READ), json!({}), agy_options()),
    ];
    for request in cases {
        let (rig, _) = ask(request.clone(), 1).await;
        assert_eq!(rig.sheets().len(), 1, "{request}");
        assert_eq!(once_option(&rig).as_deref(), Some("r-once"), "{request}");
    }
}

#[tokio::test]
async fn listing_our_own_servers_resources_is_answered_once_and_nothing_wider_is() {
    let list = mcp_request("other", None, json!({"ServerName": "quire"}), agy_options());
    let (rig, _) = ask(list, 0).await;
    assert_eq!(once_option(&rig).as_deref(), Some("a-once"));
    assert!(rig.sheets().is_empty());
    for raw in [
        json!({"ServerName": "elsewhere"}),
        json!({"ServerName": "quire", "Uri": "file:///x"}),
    ] {
        let request = mcp_request("other", None, raw.clone(), agy_options());
        let (rig, _) = ask(request, 1).await;
        assert_eq!(rig.sheets().len(), 1, "{raw}");
    }
}

#[tokio::test]
async fn the_edge_still_gates_the_call_that_follows_the_door() {
    let request = mcp_request("other", ours(READ), json!({}), agy_options());
    let (mut rig, _dir) = with_mail(
        vec![vec![
            call("perm", ASK, request),
            bridge_call("read", READ, json!({"target": thread()})),
            Act::Stop("end_turn"),
        ]],
        // The person refuses the one sheet: the call's own, at the edge.
        vec![no()],
    )
    .await;
    run_turn(&mut rig, "read the lunch mail").await;
    assert_eq!(once_option(&rig).as_deref(), Some("a-once"));
    let reply = rig.agent.reply("read").expect("reply");
    assert!(reply.get("failed").is_some(), "{reply}");
    assert_eq!(rig.sheets().len(), 1, "one sheet for the one action");
}
