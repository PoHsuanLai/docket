//! The edge end to end, in process: an rmcp client over a duplex pipe, the edge as its server,
//! the router and the fake mail app behind it. No bus, no process, no network.

use actions_mcp::*;
use docket_client::{InProcess, Intents, Transport, TransportError};
use docket_core::*;
use docket_fake::{FakeSeams, MailThread, fake_router};
use docket_router::{GrantStore, Router};
use porter_core::consent::{Decision, Grant, GrantScope, Usage};
use porter_core::{AppId, AppName, Isolation};
use prov::{ClientName, DataClass, Integrity, Source, SpaceId, SpaceScope, UnixSeconds};
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::{RoleClient, ServiceExt, service::RunningService};
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

const CLIENT: &str = "org.example.ClaudeDesktop";

/// The in-process transport, remembering every request it carried.
struct Recording {
    inner: InProcess<FakeSeams>,
    seen: Arc<Mutex<Vec<IntentsRequest>>>,
}

impl Transport for Recording {
    async fn call(&self, request: IntentsRequest) -> Result<IntentsReply, TransportError> {
        if let Ok(mut seen) = self.seen.lock() {
            seen.push(request.clone());
        }
        self.inner.call(request).await
    }
}

struct Rig {
    router: Arc<Router<FakeSeams>>,
    seen: Arc<Mutex<Vec<IntentsRequest>>>,
    client: RunningService<RoleClient, ()>,
}

fn mcp_caller() -> CallerId {
    CallerId {
        app: AppId {
            name: AppName::parse(CLIENT).expect("app"),
            isolation: Isolation::Unsandboxed,
        },
        roles: BTreeSet::from([CallerRole::Mcp]),
    }
}

fn router() -> Arc<Router<FakeSeams>> {
    let router = fake_router(AgentConfig::default()).expect("router");
    for (key, subject, body) in [("t1", "Invoice", "Pay now"), ("t2", "Digest", "This week")] {
        router.seams.link.mail.add_thread(MailThread {
            key: key.into(),
            subject: subject.into(),
            from: "eve@evil.test".into(),
            body: body.into(),
        });
    }
    Arc::new(router)
}

fn allow_mail_for_the_client(router: &Router<FakeSeams>) {
    let client = ClientName::parse(CLIENT).expect("client");
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
}

async fn rig(expose: McpExpose) -> Rig {
    rig_with(|edge| edge.with_expose(expose)).await
}

async fn rig_with(configure: impl FnOnce(McpEdge<Recording>) -> McpEdge<Recording>) -> Rig {
    let router = router();
    allow_mail_for_the_client(&router);
    let seen = Arc::new(Mutex::new(vec![]));
    let transport = Recording {
        inner: InProcess::new(router.clone(), mcp_caller()),
        seen: seen.clone(),
    };
    let edge = configure(McpEdge::new(
        Intents::over(transport),
        ClientName::parse(CLIENT).expect("client"),
    ));
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        if let Ok(running) = edge.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });
    let client = ().serve(client_io).await.expect("client connects");
    Rig {
        router,
        seen,
        client,
    }
}

fn thread(key: &str) -> Json {
    json!({ "app": "org.quire.Mail", "kind": "mail.thread", "key": key })
}

async fn call(rig: &Rig, tool: &str, arguments: Json) -> CallToolResult {
    let mut params = CallToolRequestParams::new(tool.to_owned());
    if let Json::Object(map) = arguments {
        params = params.with_arguments(map);
    }
    rig.client.call_tool(params).await.expect("a tool result")
}

fn text_of(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn performs(rig: &Rig) -> Vec<CallRequest> {
    rig.seen
        .lock()
        .expect("log")
        .iter()
        .filter_map(|r| match r {
            IntentsRequest::Perform { call, .. } => Some(call.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn an_edge_nobody_switched_on_lists_nothing_and_refuses_every_call() {
    assert_eq!(McpExpose::default(), McpExpose::Off);
    let rig = rig(McpExpose::default()).await;
    assert!(rig.client.list_all_tools().await.expect("list").is_empty());
    let result = call(
        &rig,
        "mail__mail_thread_read",
        json!({ "target": thread("t1") }),
    )
    .await;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(text_of(&result), McpFault::Off.to_string());
    assert!(performs(&rig).is_empty(), "the router was never asked");
}

#[tokio::test]
async fn the_tools_are_the_offered_actions_with_hints_and_a_target_key() {
    let rig = rig(McpExpose::On).await;
    let tools = rig.client.list_all_tools().await.expect("list");
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert!(names.contains(&"mail__mail_thread_read"), "{names:?}");
    assert!(names.contains(&"mail__mail_thread_delete"));
    let read = tools
        .iter()
        .find(|t| t.name == "mail__mail_thread_read")
        .expect("read");
    assert_eq!(
        read.annotations.as_ref().and_then(|a| a.read_only_hint),
        Some(true)
    );
    let target = &read.input_schema["properties"]["target"];
    assert_eq!(target["properties"]["kind"]["const"], "mail.thread");
    assert_eq!(read.input_schema["required"], json!(["target"]));
    let delete = tools
        .iter()
        .find(|t| t.name == "mail__mail_thread_delete")
        .expect("delete");
    assert_eq!(
        delete.annotations.as_ref().and_then(|a| a.destructive_hint),
        Some(true)
    );
}

#[tokio::test]
async fn a_read_is_allowed_and_its_arguments_reach_the_router_untrusted() {
    let rig = rig(McpExpose::On).await;
    let result = call(
        &rig,
        "mail__mail_thread_read",
        json!({ "target": thread("t1") }),
    )
    .await;
    assert_eq!(result.is_error, Some(false), "{}", text_of(&result));
    let body: Json = serde_json::from_str(&text_of(&result)).expect("json");
    assert_eq!(body["value"]["v"], "Pay now");
    let calls = performs(&rig);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].origin, Origin::Mcp);
    assert_eq!(calls[0].action.name.as_str(), "mail.thread.read");
}

#[tokio::test]
async fn a_write_asks_and_nothing_runs_when_the_person_does_not_answer() {
    let rig = rig(McpExpose::On).await;
    let result = call(
        &rig,
        "mail__mail_thread_archive",
        json!({ "target": [thread("t1")] }),
    )
    .await;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(text_of(&result), "refused: not confirmed");
    assert!(!rig.router.seams.link.mail.is_archived("t1"));
    assert_eq!(
        rig.router.seams.confirmer.requests().len(),
        1,
        "the sheet was asked, by the router, not answered by the edge"
    );
    let destroy = call(
        &rig,
        "mail__mail_thread_delete",
        json!({ "target": [thread("t2")] }),
    )
    .await;
    assert_eq!(text_of(&destroy), "refused: not confirmed");
    assert!(rig.router.seams.link.mail.has_thread("t2"));
}

#[tokio::test]
async fn every_argument_is_sent_untrusted_from_this_client() {
    let rig = rig(McpExpose::On).await;
    let _ = call(
        &rig,
        "mail__mail_draft_create",
        json!({ "body": "Dear Eve" }),
    )
    .await;
    let calls = performs(&rig);
    assert_eq!(calls.len(), 1);
    assert!(!calls[0].args.is_empty());
    for held in calls[0].args.values() {
        assert_eq!(held.label.integrity, Integrity::Untrusted);
        assert!(matches!(
            held.label.sources.iter().next(),
            Some(Source::Mcp(client)) if client.as_str() == CLIENT
        ));
    }
}

#[tokio::test]
async fn bad_arguments_and_unknown_tools_never_reach_the_router() {
    let rig = rig(McpExpose::On).await;
    let cases = [
        (
            "mail__mail_thread_read",
            json!({}),
            "target: the action needs a target",
        ),
        (
            "mail__mail_thread_read",
            json!({ "target": thread("t1"), "extra": 1 }),
            "unknown argument \"extra\"",
        ),
        (
            "mail__mail_draft_create",
            json!({}),
            "missing argument body",
        ),
        (
            "mail__mail_draft_create",
            json!({ "body": 7 }),
            "argument body: wrong type",
        ),
        ("nothing__here", json!({}), "unknown tool"),
        ("memory__memory_forget", json!({}), "unknown tool"),
    ];
    for (tool, arguments, why) in cases {
        let result = call(&rig, tool, arguments).await;
        assert_eq!(result.is_error, Some(true), "{tool}");
        assert_eq!(text_of(&result), why, "{tool}");
    }
    assert!(performs(&rig).is_empty());
}

#[tokio::test]
async fn a_refusal_names_a_kind_and_never_a_reason() {
    let rig = rig(McpExpose::On).await;
    // The client holds no grant for Files: the router refuses, and the edge says only "denied".
    let result = call(
        &rig,
        "files__files_file_read",
        json!({ "target": { "app": "org.quire.Files", "kind": "files.file", "key": "f1" } }),
    )
    .await;
    assert_eq!(result.is_error, Some(true));
    let said = text_of(&result);
    assert!(said.starts_with("refused:"), "{said}");
    for leak in ["cedar", "policy", "rule", "review"] {
        assert!(!said.to_lowercase().contains(leak), "{said}");
    }
}

#[tokio::test]
async fn call_can_be_used_without_a_server() {
    let router = router();
    allow_mail_for_the_client(&router);
    let edge = McpEdge::new(
        Intents::over(InProcess::new(router, mcp_caller())),
        ClientName::parse(CLIENT).expect("client"),
    );
    assert_eq!(
        edge.call("mail__mail_thread_read", json!({ "target": thread("t1") }))
            .await,
        Err(McpFault::Off)
    );
    let edge = edge.with_expose(McpExpose::On);
    assert_eq!(edge.expose(), McpExpose::On);
    assert_eq!(
        edge.call("mail__mail_thread_read", json!([1])).await,
        Err(McpFault::Args(ArgsFault::NotAnObject))
    );
}

#[tokio::test]
async fn an_edge_following_the_settings_file_switches_on_and_off_with_it() {
    use docket_settings::{AgentSettings, Locator};
    let home = tempfile::tempdir().expect("scratch");
    let env = |key: &str| match key {
        "XDG_CONFIG_HOME" => Some(home.path().display().to_string()),
        "XDG_CONFIG_DIRS" => Some(home.path().join("none").display().to_string()),
        _ => None,
    };
    let locator = Locator::from_env(&env);
    let rig = rig_with(|edge| {
        edge.with_expose_read(move || locator.read(AgentSettings::default()).value.expose)
    })
    .await;
    let write = |text: &str| {
        std::fs::create_dir_all(home.path().join("docket")).expect("dir");
        let temp = home.path().join("docket/settings.toml.tmp");
        std::fs::write(&temp, text).expect("write");
        std::fs::rename(&temp, home.path().join("docket/settings.toml")).expect("rename");
    };
    // No file: off.
    assert!(rig.client.list_all_tools().await.expect("list").is_empty());
    // The person switches it on in the Settings app: the next request sees the tools.
    write("[agent.mcp]\nexpose = \"on\"\n");
    let tools = rig.client.list_all_tools().await.expect("list");
    assert!(!tools.is_empty());
    // And off again: no tools, and a call is refused before the router is asked.
    write("[agent.mcp]\nexpose = \"off\"\n");
    assert!(rig.client.list_all_tools().await.expect("list").is_empty());
    let result = call(
        &rig,
        "mail__mail_thread_read",
        json!({ "target": thread("t1") }),
    )
    .await;
    assert_eq!(text_of(&result), McpFault::Off.to_string());
    assert!(performs(&rig).is_empty());
}
