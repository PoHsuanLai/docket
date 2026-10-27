//! The per-session tool edge, what an agent can send it: an edited entry, lines that are not
//! requests, and the menu a helper's agent is offered.

use super::agent::{Act, bridge_list};
use super::edge::{offer, reached, thread, with_mail};
use super::rig::{Rig, program, run_turn};
use docket_acp_host::{EdgeMenu, ToolsEdge};
use docket_core::{ActionMatch, LabelText, TaskPolicy, TaskPolicyState};
use prov::{Effect, SessionId, SpaceId, TaskId, UnixSeconds};
use serde_json::{Value, json};

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

fn reach_of(app: &str) -> TaskPolicy {
    TaskPolicy {
        task: TaskId::parse("t-0").expect("task"),
        space: SpaceId::parse("work").expect("space"),
        from: Vec::new(),
        actions: std::collections::BTreeSet::from([ActionMatch::AppUpTo(
            porter_core::AppName::parse(app).expect("app"),
            Effect::Read,
        )]),
        kinds: std::collections::BTreeSet::new(),
        ceiling: Effect::Read,
        max_count: porter_core::Count(1),
        recipients: Vec::new(),
        destinations: Vec::new(),
        paths: Vec::new(),
        expires: UnixSeconds(i64::MAX),
        rationale: LabelText::parse("what the helper may use").expect("words"),
        state: TaskPolicyState::Active,
    }
}

/// The names a helper's edge lists for `app`'s reach, and what a call to a mail tool gets.
async fn helper_menu(
    rig: &Rig<super::rig::Fakes>,
    dir: &tempfile::TempDir,
    app: &str,
) -> (Vec<String>, Value) {
    let session = SessionId::parse("s-98").expect("session");
    let menu = EdgeMenu::Helper(Box::new(reach_of(app)));
    let edge = ToolsEdge::start_with(&offer(dir), &session, &program(), rig.court.clone(), menu)
        .expect("edge");
    let socket = edge.bind().socket.as_str().to_owned();
    let line = |op: Value| json!({"token": edge.token().reveal(), "op": op});
    let listed = super::agent::over_socket(&socket, &line(json!("list")))
        .await
        .expect("list");
    let names = listed["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|t| t["tool"]["name"].as_str().map(str::to_owned))
        .collect();
    let read =
        json!({"call": {"tool": "mail__mail_thread_read", "arguments": {"target": thread()}}});
    let called = super::agent::over_socket(&socket, &line(read))
        .await
        .expect("call");
    (names, called)
}

/// Why: a helper's agent app is offered what a native helper is: its card's apps, not the rest.
#[tokio::test]
async fn a_helpers_agent_is_offered_its_cards_actions_and_not_another_apps() {
    let (rig, dir) = with_mail(Vec::new(), Vec::new()).await;
    let (names, _) = helper_menu(&rig, &dir, "org.quire.Mail").await;
    assert!(
        names.iter().any(|n| n == "mail__mail_thread_read"),
        "{names:?}"
    );
    let (names, called) = helper_menu(&rig, &dir, "org.quire.Notes").await;
    assert!(!names.iter().any(|n| n.starts_with("mail__")), "{names:?}");
    assert!(called.to_string().contains("unknown_tool"), "{called}");
    assert_eq!(reached(&rig), 0);
}
