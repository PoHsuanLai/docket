//! `session/list` and `session/load` honour the opener rule: an editor sees and restores only
//! the sessions it opened, and cannot tell another app's session from one that does not exist.

use crate::support::*;
use docket_core::DenyCode;
use docket_session::fake::MemoryLog;
use docket_session::{BackendKind, Opening, Seq, SessionEntry, SessionLog, TurnEnd, Workspace};
use prov::{AgentRef, Effect, SessionId, SpaceId, TaskId};
use serde_json::{Value, json};
use std::sync::Arc;

const ZED: &str = "acp.zed";
const OTHER: &str = "acp.other";

fn script() -> Vec<Vec<Vec<docket_session::BackendEvent>>> {
    vec![vec![vec![
        words("Searching."),
        started(1, "mail.search", Effect::Read),
        step(1, "mail.search", Effect::Read, done("2 found")),
        started(2, "mail.send", Effect::Outbound),
        step(
            2,
            "mail.send",
            Effect::Outbound,
            denied(DenyCode::NeedsUser),
        ),
        end(TurnEnd::Done),
    ]]]
}

async fn opening_by(log: &MemoryLog, session: &str, opener: Option<&str>) {
    let opening = Opening {
        task: TaskId::parse(&format!("t-{session}")).unwrap(),
        space: SpaceId::desktop(),
        opener: opener.map(app),
        agent: Some(AgentRef::Companion),
        backend: BackendKind::Native,
        parent: None,
        forked_from: None,
        cwd: Some(Workspace::parse("/work/a").unwrap()),
        started_from: None,
    };
    let id = SessionId::parse(session).unwrap();
    log.append(&id, Seq(0), &SessionEntry::Opened(opening))
        .await
        .unwrap();
}

/// Zed opens and uses one session in `/work/a`; the log then holds it, and two foreign ones.
async fn seeded() -> Arc<MemoryLog> {
    let log = Arc::new(MemoryLog::new());
    let (mut server, mut ed) = rig(&log, script(), ZED);
    let run = async move {
        ed.initialize().await;
        let session = ed.new_session("/work/a").await;
        assert_eq!(session, "s-1");
        ed.prompt(
            &session,
            "find the invoices and mail them to Eve",
            &mut allow,
        )
        .await;
    };
    let (ran, ()) = tokio::join!(server.run(), run);
    assert!(ran.is_ok());
    opening_by(&log, "s-50", Some("org.quire.Shell")).await;
    opening_by(&log, "s-51", None).await;
    opening_by(&log, "s-52", Some(OTHER)).await;
    log
}

#[tokio::test]
async fn list_shows_an_editor_only_the_sessions_it_opened() {
    let log = seeded().await;
    let (mut zed, mut ed) = rig(&log, vec![], ZED);
    let mine = async move {
        ed.initialize().await;
        let all = ed
            .call(
                "session/list",
                json!({}),
                "ListSessionsRequest",
                "ListSessionsResponse",
            )
            .await;
        let rows = all["result"]["sessions"].as_array().unwrap().clone();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0]["sessionId"], "s-1");
        assert_eq!(rows[0]["cwd"], "/work/a");
        assert_eq!(rows[0]["title"], "find the invoices and mail them to Eve");
        assert_eq!(rows[0]["updatedAt"], "2025-10-09T08:53:20Z");
        let elsewhere = ed
            .call(
                "session/list",
                json!({"cwd": "/work/b"}),
                "ListSessionsRequest",
                "ListSessionsResponse",
            )
            .await;
        assert!(
            elsewhere["result"]["sessions"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let there = ed
            .call(
                "session/list",
                json!({"cwd": "/work/a"}),
                "ListSessionsRequest",
                "ListSessionsResponse",
            )
            .await;
        assert_eq!(there["result"]["sessions"].as_array().unwrap().len(), 1);
    };
    let (ran, ()) = tokio::join!(zed.run(), mine);
    assert!(ran.is_ok());

    let (mut other, mut ed) = rig(&log, vec![], OTHER);
    let theirs = async move {
        ed.initialize().await;
        let all = ed
            .call(
                "session/list",
                json!({}),
                "ListSessionsRequest",
                "ListSessionsResponse",
            )
            .await;
        let rows = all["result"]["sessions"].as_array().unwrap().clone();
        let ids: Vec<&str> = rows
            .iter()
            .map(|r| r["sessionId"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["s-52"], "only its own");
    };
    let (ran, ()) = tokio::join!(other.run(), theirs);
    assert!(ran.is_ok());
}

#[tokio::test]
async fn load_replays_for_the_opener_and_answers_everyone_else_as_unknown() {
    let log = seeded().await;
    // A restart: a new server and host over the same log.
    let (mut zed, mut ed) = rig(&log, vec![vec![vec![words("Back.")]]], ZED);
    let run = async move {
        ed.initialize().await;
        let load = |s: &str, cwd: &str| json!({"sessionId": s, "cwd": cwd, "mcpServers": []});
        let wrong = ed
            .call(
                "session/load",
                load("s-1", "/work/elsewhere"),
                "LoadSessionRequest",
                "LoadSessionResponse",
            )
            .await;
        assert_eq!(wrong["error"]["code"], -32602);
        let before = ed.seen.len();
        let ok = ed
            .call(
                "session/load",
                load("s-1", "/work/a"),
                "LoadSessionRequest",
                "LoadSessionResponse",
            )
            .await;
        assert!(ok.get("result").is_some(), "{ok}");
        let replayed: Vec<Value> = ed.seen[before..]
            .iter()
            .filter(|m| m["method"] == "session/update")
            .map(|m| m["params"]["update"].clone())
            .collect();
        let kinds: Vec<&str> = replayed
            .iter()
            .map(|u| u["sessionUpdate"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            [
                "user_message_chunk",
                "tool_call",
                "tool_call_update",
                "tool_call",
                "tool_call_update"
            ]
        );
        assert_eq!(
            replayed[0]["content"]["text"],
            "find the invoices and mail them to Eve"
        );
        assert_eq!(replayed[4]["status"], "failed");
        // Another app's, a legacy one with no opener, the shell's, and one that never was:
        // the same answer.
        let mut answers = Vec::new();
        for id in ["s-50", "s-51", "s-52", "s-99"] {
            let reply = ed
                .call(
                    "session/load",
                    load(id, "/work/a"),
                    "LoadSessionRequest",
                    "LoadSessionResponse",
                )
                .await;
            answers.push(reply["error"].clone());
        }
        assert!(answers.iter().all(|a| a == &answers[0]), "{answers:?}");
        assert_eq!(answers[0]["message"], "no such session");
        // And the loaded session goes on.
        let reply = ed.prompt("s-1", "and again", &mut allow).await;
        assert_eq!(reply["result"]["stopReason"], "end_turn");
    };
    let (ran, ()) = tokio::join!(zed.run(), run);
    assert!(ran.is_ok());
}
