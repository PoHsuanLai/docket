//! The per-session tool edge: an external agent calls the desktop's actions through an MCP server
//! the host offered it, and every call is a router call in the session the host opened for that
//! agent, as that program. The fake agent plays the bridge program: it reads the server entry it
//! was offered and speaks to the socket named there, as the real bridge does.

use super::agent::{Act, Offered, bridge_call, bridge_list};
use super::rig::{Rig, Setup, abs, always, ended_with, no, once, program, run_turn, started};
use docket_acp::client::{ToolsEdge, ToolsOffer};
use docket_core::{AuditRecord, GrantCaller, Saw};
use docket_fake::{MailContact, MailThread};
use docket_session::{SessionHost, TurnEnd};
use prov::{Actor, SessionId};
use serde_json::{Value, json};

const THREAD: &str = "t1";

fn offer(dir: &tempfile::TempDir) -> ToolsOffer {
    ToolsOffer {
        run_dir: dir.path().to_path_buf(),
        bridge: abs("/opt/docket/actions-mcp"),
    }
}

pub(super) fn thread() -> Value {
    json!({"app": "org.quire.Mail", "kind": "mail.thread", "key": THREAD})
}

pub(super) async fn with_mail(
    turns: Vec<Vec<Act>>,
    answers: Vec<docket_core::ConfirmAnswer>,
) -> (Rig<super::rig::Fakes>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("scratch");
    let (rig, _files) = started(Setup {
        turns,
        answers,
        tools: Some(offer(&dir)),
        ..Setup::default()
    })
    .await;
    rig.router.seams.link.mail.add_thread(MailThread {
        key: THREAD.into(),
        subject: "Lunch".into(),
        from: "ann@example.org".into(),
        body: "Noon at the usual place.".into(),
    });
    rig.router.seams.link.mail.add_contact(MailContact {
        key: "ann".into(),
        name: "Ann".into(),
        address: "ann@example.org".into(),
    });
    (rig, dir)
}

fn tool_calls(rig: &Rig<super::rig::Fakes>, action: &str) -> Vec<Actor> {
    rig.audit()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Call {
                actor, action: a, ..
            } if a.name.as_str() == action => Some(actor),
            _ => None,
        })
        .collect()
}

fn offered(rig: &Rig<super::rig::Fakes>) -> Offered {
    let params = rig.agent.new_session();
    Offered::from_params(&params).expect("a server was offered")
}

#[tokio::test]
async fn session_new_offers_one_stdio_server_whose_token_is_not_on_a_command_line() {
    let (rig, _dir) = with_mail(Vec::new(), Vec::new()).await;
    let servers = rig.agent.new_session()["mcpServers"].clone();
    assert_eq!(servers.as_array().map(Vec::len), Some(1));
    let server = offered(&rig);
    assert_eq!(server.name, "quire");
    assert_eq!(server.command, "/opt/docket/actions-mcp");
    assert_eq!(server.args.len(), 2);
    assert_eq!(server.args[0], "--host-socket");
    assert_eq!(server.token().len(), 64);
    assert!(
        server.args.iter().all(|a| !a.contains(&server.token())),
        "the token is in the environment only"
    );
    // The launcher is told what to bind in.
    let plans = rig.spawned.plans();
    let bind = plans[0].edge.as_ref().expect("an edge bind");
    assert_eq!(bind.socket.as_str(), server.socket());
    assert_eq!(bind.bridge.as_str(), "/opt/docket/actions-mcp");
}

#[tokio::test]
async fn a_host_with_no_edge_offers_the_agent_no_server() {
    let (rig, _files) = started(Setup::default()).await;
    assert_eq!(rig.agent.new_session()["mcpServers"], json!([]));
    assert!(rig.spawned.plans()[0].edge.is_none());
}

#[tokio::test]
async fn the_tools_are_the_mcp_edges_and_never_the_hosts_own_actions() {
    let (mut rig, _dir) = with_mail(
        vec![vec![bridge_list("list"), Act::Stop("end_turn")]],
        vec![],
    )
    .await;
    run_turn(&mut rig, "what can you do").await;
    let reply = rig.agent.reply("list").expect("a reply");
    let names: Vec<&str> = reply["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|t| t["tool"]["name"].as_str())
        .collect();
    assert!(names.contains(&"mail__mail_thread_read"), "{names:?}");
    assert!(names.contains(&"mail__mail_message_send"), "{names:?}");
    assert!(
        !names.iter().any(|n| n.starts_with("acpagent")),
        "{names:?}"
    );
    // What is hidden from a planner or an MCP client is hidden here.
    assert!(!names.contains(&"mail__mail_thread_open"), "{names:?}");
}

#[tokio::test]
async fn a_read_is_a_router_call_in_the_agents_session_as_that_program() {
    let (mut rig, _dir) = with_mail(
        vec![vec![
            bridge_call(
                "read",
                "mail__mail_thread_read",
                json!({"target": thread()}),
            ),
            Act::Stop("end_turn"),
        ]],
        // The first use of a data class asks, as it does for the planner.
        vec![once()],
    )
    .await;
    let events = run_turn(&mut rig, "read the lunch mail").await;
    assert_eq!(*ended_with(&events), TurnEnd::Done);
    let reply = rig.agent.reply("read").expect("a reply");
    assert!(reply.get("done").is_some(), "{reply}");
    assert!(
        reply.to_string().contains("Noon"),
        "the content came back: {reply}"
    );
    let actors = tool_calls(&rig, "mail.thread.read");
    assert_eq!(actors.len(), 1, "one router call");
    assert!(
        matches!(&actors[0], Actor::Acp { program: p, .. } if p.as_str() == program().as_str()),
        "{actors:?}"
    );
    // The content was untrusted: the session is tainted by it, as for any caller.
    let saw = rig.router.state.lock().expect("lock").sessions[&rig.session]
        .saw
        .untrusted;
    assert_eq!(saw, Saw::Seen, "the router took the taint");
}

#[tokio::test]
async fn a_write_asks_the_person_and_any_grant_is_the_programs_in_dockets_store() {
    let (mut rig, _dir) = with_mail(
        vec![vec![
            bridge_call(
                "archive",
                "mail__mail_thread_archive",
                json!({"target": [thread()]}),
            ),
            Act::Stop("end_turn"),
        ]],
        vec![always(), always()],
    )
    .await;
    run_turn(&mut rig, "archive the lunch mail").await;
    let reply = rig.agent.reply("archive").expect("reply");
    assert!(reply.get("done").is_some(), "{reply}");
    assert!(rig.router.seams.link.mail.is_archived(THREAD));
    assert!(!rig.sheets().is_empty(), "the person was asked");
    // Whatever the person granted belongs to this program (this action, outside the task, offers
    // no standing grant, so there may be none), held where Settings lists it.
    let held = docket_router::GrantStore::grants(&rig.router.seams.grants);
    assert!(
        held.iter()
            .all(|g| g.key.caller == GrantCaller::AcpAgent(program())),
        "{held:?}"
    );
}

#[tokio::test]
async fn a_refusal_comes_back_coarse_and_nothing_is_done() {
    let (mut rig, _dir) = with_mail(
        vec![vec![
            bridge_call(
                "archive",
                "mail__mail_thread_archive",
                json!({"target": [thread()]}),
            ),
            Act::Stop("end_turn"),
        ]],
        vec![no()],
    )
    .await;
    run_turn(&mut rig, "archive the lunch mail").await;
    let reply = rig.agent.reply("archive").expect("reply");
    assert!(reply.get("failed").is_some(), "{reply}");
    assert!(!rig.router.seams.link.mail.is_archived(THREAD));
}

#[tokio::test]
async fn five_refusals_in_a_row_pause_the_turn_as_the_routers_breaker_does_for_any_agent() {
    let acts = (0..6)
        .map(|n| {
            let tag: &'static str = Box::leak(format!("a{n}").into_boxed_str());
            bridge_call(
                tag,
                "mail__mail_thread_archive",
                json!({"target": [thread()]}),
            )
        })
        .chain([Act::Stop("end_turn")])
        .collect();
    let (mut rig, _dir) = with_mail(vec![acts], vec![no(); 6]).await;
    let events = run_turn(&mut rig, "archive").await;
    assert!(!rig.router.seams.link.mail.is_archived(THREAD));
    let breaker = rig
        .audit()
        .iter()
        .any(|r| matches!(r, AuditRecord::Breaker { .. }));
    assert!(breaker, "{:?}", rig.audit());
    assert!(
        matches!(ended_with(&events), TurnEnd::Paused(_)),
        "{events:?}"
    );
}

// ---- the hostile cases ----

fn reached(rig: &Rig<super::rig::Fakes>) -> usize {
    rig.audit()
        .iter()
        .filter(|r| matches!(r, AuditRecord::Call { .. }))
        .count()
}

/// Why: the token is the session's. A bridge started with a token it made up, or a stale one, is a
/// stranger: it is answered `refused` and the router sees nothing.
#[tokio::test]
async fn a_wrong_token_is_refused_and_reaches_nothing() {
    let (mut rig, _dir) = with_mail(
        vec![vec![
            Act::Bridge {
                tag: "wrong",
                make: Box::new(|o| (o.socket(), json!({"token": "0".repeat(64), "op": "list"}))),
            },
            Act::Bridge {
                tag: "short",
                make: Box::new(|o| (o.socket(), json!({"token": "abc", "op": "list"}))),
            },
            Act::Bridge {
                tag: "none",
                make: Box::new(|o| (o.socket(), json!({"op": "list"}))),
            },
            Act::Stop("end_turn"),
        ]],
        vec![],
    )
    .await;
    run_turn(&mut rig, "go").await;
    for tag in ["wrong", "short", "none"] {
        assert_eq!(
            rig.agent.reply(tag).expect("reply"),
            json!("refused"),
            "{tag}"
        );
    }
    assert_eq!(reached(&rig), 0);
}

/// Why: nothing the agent writes may name a session, a program or an app. A request with such a
/// field is not a request; a tool argument of that name is an argument the action does not have.
#[tokio::test]
async fn a_call_cannot_name_another_session_program_or_app() {
    let (mut rig, _dir) = with_mail(
        vec![vec![
            Act::Bridge {
                tag: "session",
                make: Box::new(|o| {
                    let (socket, mut line) = o.line(json!({"call": {"tool": "mail__mail_thread_read", "arguments": {"target": thread()}}}));
                    line["session"] = json!("s-9");
                    (socket, line)
                }),
            },
            Act::Bridge {
                tag: "program",
                make: Box::new(|o| {
                    o.line(json!({"call": {"tool": "mail__mail_thread_read", "program": "gemini-cli", "arguments": {"target": thread()}}}))
                }),
            },
            bridge_call("arg", "mail__mail_thread_read", json!({"target": thread(), "session": "s-9", "program": "gemini-cli"})),
            Act::Stop("end_turn"),
        ]],
        vec![],
    )
    .await;
    run_turn(&mut rig, "go").await;
    assert_eq!(rig.agent.reply("session").expect("reply"), json!("refused"));
    assert_eq!(rig.agent.reply("program").expect("reply"), json!("refused"));
    let reply = rig.agent.reply("arg").expect("reply");
    assert!(
        reply.get("failed").is_some(),
        "an argument the action lacks is refused: {reply}"
    );
    assert_eq!(reached(&rig), 0, "nothing reached the router as anyone");
}

/// Why: the host's own pseudo-app is not a tool, and a tool name that is not offered is unknown,
/// however it is spelled.
#[tokio::test]
async fn the_hosts_actions_and_hidden_ones_are_not_tools() {
    let (mut rig, _dir) = with_mail(
        vec![vec![
            bridge_call(
                "write",
                "acpagent__files_write",
                json!({"target": ["/work/app/x"]}),
            ),
            bridge_call(
                "hidden",
                "mail__mail_thread_open",
                json!({"target": [thread()]}),
            ),
            bridge_call("dots", "mail.thread.read", json!({"target": [thread()]})),
            Act::Stop("end_turn"),
        ]],
        vec![],
    )
    .await;
    run_turn(&mut rig, "go").await;
    for tag in ["write", "hidden", "dots"] {
        let reply = rig.agent.reply(tag).expect("reply");
        assert!(reply.to_string().contains("unknown_tool"), "{tag}: {reply}");
    }
    assert_eq!(reached(&rig), 0);
}

/// Why: a socket serves the session it was made for. An edge for a session the host never opened
/// answers its own token and still gets nothing from the router.
#[tokio::test]
async fn an_edge_cannot_act_in_a_session_the_host_did_not_open_and_tokens_are_not_shared() {
    let (mut rig, dir) = with_mail(vec![vec![Act::Stop("end_turn")]], vec![]).await;
    run_turn(&mut rig, "go").await;
    let stranger = SessionId::parse("s-99").expect("session");
    let other =
        ToolsEdge::start(&offer(&dir), &stranger, &program(), rig.court.clone()).expect("edge");
    let mine = offered(&rig);
    let theirs = Offered {
        args: vec!["--host-socket".into(), other.bind().socket.as_str().into()],
        env: [(
            "QUIRE_EDGE_TOKEN".to_owned(),
            other.token().reveal().to_owned(),
        )]
        .into(),
        ..Offered::default()
    };
    let read =
        json!({"call": {"tool": "mail__mail_thread_read", "arguments": {"target": thread()}}});
    // This session's token on the other socket, and the other's on this one: both strangers.
    let crossed = [
        (
            theirs.socket(),
            json!({"token": mine.token(), "op": read.clone()}),
        ),
        (
            mine.socket(),
            json!({"token": theirs.token(), "op": read.clone()}),
        ),
    ];
    for (socket, line) in crossed {
        assert_eq!(
            super::agent::over_socket(&socket, &line).await,
            Ok(json!("refused"))
        );
    }
    // The other edge's own token is good for its own socket, and the router refuses the session.
    let (socket, line) = theirs.line(read);
    let reply = super::agent::over_socket(&socket, &line)
        .await
        .expect("reply");
    assert!(reply.get("failed").is_some(), "{reply}");
    assert_eq!(reached(&rig), 0);
    drop(other);
}

/// Why: when the session closes the capability dies with it. The socket is gone, a later connect
/// finds nothing, and nothing reaches the router.
#[tokio::test]
async fn after_the_session_is_closed_the_edge_is_gone() {
    let (mut rig, _dir) = with_mail(
        vec![vec![bridge_list("before"), Act::Stop("end_turn")]],
        vec![],
    )
    .await;
    run_turn(&mut rig, "go").await;
    assert!(
        rig.agent
            .reply("before")
            .expect("reply")
            .get("tools")
            .is_some()
    );
    let server = offered(&rig);
    let before = reached(&rig);
    let session = rig.session.clone();
    rig.host
        .close(&session, docket_session::EndCause::Closed)
        .await
        .expect("close");
    let (socket, line) = server.line(
        json!({"call": {"tool": "mail__mail_thread_read", "arguments": {"target": thread()}}}),
    );
    assert_eq!(
        super::agent::over_socket(&socket, &line).await,
        Err(json!("unreachable"))
    );
    assert!(!std::path::Path::new(&socket).exists());
    assert_eq!(reached(&rig), before);
}

/// Why: the agent can rewrite the entry it was given (a different command, arguments or
/// environment). That changes which program it runs, never whose session the calls are in:
/// the program still holds only this session's socket and token.
#[tokio::test]
async fn an_agent_that_edits_the_entry_it_was_given_gains_nothing() {
    let (mut rig, _dir) = with_mail(
        vec![vec![
            // A different socket path, the token as given.
            Act::Bridge {
                tag: "path",
                make: Box::new(|o| {
                    let (_, line) = o.line(json!("list"));
                    (format!("{}x", o.socket()), line)
                }),
            },
            // The token cut, extended, upper-cased, or taken from the arguments.
            Act::Bridge {
                tag: "upper",
                make: Box::new(|o| {
                    (
                        o.socket(),
                        json!({"token": o.token().to_uppercase(), "op": "list"}),
                    )
                }),
            },
            Act::Bridge {
                tag: "longer",
                make: Box::new(|o| {
                    (
                        o.socket(),
                        json!({"token": format!("{}0", o.token()), "op": "list"}),
                    )
                }),
            },
            Act::Bridge {
                tag: "args",
                make: Box::new(|o| {
                    (
                        o.socket(),
                        json!({"token": o.args[0].clone(), "op": "list"}),
                    )
                }),
            },
            Act::Stop("end_turn"),
        ]],
        vec![],
    )
    .await;
    run_turn(&mut rig, "go").await;
    assert_eq!(
        rig.agent.reply("path").expect_err("no such socket"),
        json!("unreachable")
    );
    for tag in ["upper", "longer", "args"] {
        assert_eq!(
            rig.agent.reply(tag).expect("reply"),
            json!("refused"),
            "{tag}"
        );
    }
    assert_eq!(reached(&rig), 0);
}

/// Why: a line that is not a request, or is far too long, closes nothing else and does nothing.
#[tokio::test]
async fn garbage_and_oversized_lines_are_refused_without_effect() {
    let (mut rig, _dir) = with_mail(
        vec![vec![
            Act::Bridge {
                tag: "text",
                make: Box::new(|o| (o.socket(), json!("not an object"))),
            },
            Act::Bridge {
                tag: "big",
                make: Box::new(|o| {
                    o.line(
                        json!({"call": {"tool": "t", "arguments": {"x": "a".repeat(5_000_000)}}}),
                    )
                }),
            },
            bridge_list("after"),
            Act::Stop("end_turn"),
        ]],
        vec![],
    )
    .await;
    run_turn(&mut rig, "go").await;
    assert_eq!(rig.agent.reply("text").expect("reply"), json!("refused"));
    assert!(rig.agent.reply("big").is_err() || rig.agent.reply("big") == Ok(json!("refused")));
    assert!(
        rig.agent
            .reply("after")
            .expect("reply")
            .get("tools")
            .is_some()
    );
}
