//! A whole session with the fake agent: initialize, session/new, a prompt in which the agent
//! reads and writes files, asks permission, runs a command, and ends. Every call it makes is a
//! router call, ruled by the real router.

use super::agent::{AGENT_SESSION, Act, call, call_with, fire_with, say};
use super::rig::{
    CWD, Setup, abs, always, no, once, program, read, run_turn, selected, started, tool, write,
};
use crate::support::app;
use bulkhead::fake::Script;
use docket_acp::client::TaintSource;
use docket_core::{
    AuditRecord, CallerId, CallerRole, FILES_READ, FILES_WRITE, GrantCaller, IntentsReply,
    IntentsRequest, StandingScope, TERMINAL_RUN, TurnState,
};
use docket_session::{BackendEvent, CallEvent, EndCause, SessionHost, TurnEnd};
use porter_core::{AppId, Isolation};
use prov::Actor;
use serde_json::json;

fn script() -> Vec<Act> {
    vec![
        call("read_in", "fs/read_text_file", read("/work/app/README.md")),
        call("read_out", "fs/read_text_file", read("/etc/passwd")),
        call(
            "write_in",
            "fs/write_text_file",
            write("/work/app/out.txt", "hello"),
        ),
        call(
            "write_out",
            "fs/write_text_file",
            write("/work/other/x.txt", "no"),
        ),
        call(
            "perm1",
            "session/request_permission",
            tool("edit", "Edit notes", &["/work/app/notes.md"], json!({})),
        ),
        call(
            "perm2",
            "session/request_permission",
            tool("edit", "Edit more", &["/work/app/more.md"], json!({})),
        ),
        call(
            "term",
            "terminal/create",
            json!({"sessionId": AGENT_SESSION, "command": "echo", "args": ["hi"]}),
        ),
        call_with(
            "out",
            "terminal/output",
            |r| json!({"sessionId": AGENT_SESSION, "terminalId": r["term"].clone().unwrap()["terminalId"]}),
        ),
        call_with(
            "exit",
            "terminal/wait_for_exit",
            |r| json!({"sessionId": AGENT_SESSION, "terminalId": r["term"].clone().unwrap()["terminalId"]}),
        ),
        call_with(
            "release",
            "terminal/release",
            |r| json!({"sessionId": AGENT_SESSION, "terminalId": r["term"].clone().unwrap()["terminalId"]}),
        ),
        say("done"),
        Act::Stop("end_turn"),
    ]
}

#[tokio::test]
async fn a_full_session_goes_through_the_router_call_by_call() {
    let (mut rig, files) = started(Setup {
        turns: vec![script()],
        // After the file is served the session is tainted, so the router asks: the write once;
        // the first permission always (a standing grant); the command once.
        answers: vec![once(), always(), once()],
        scripts: vec![Script::done("hi\n", 0)],
        ..Setup::default()
    })
    .await;
    files.put(&abs("/work/app/README.md"), "# readme\n");
    files.put(&abs("/etc/passwd"), "root:x:0:0");

    // The handshake: file and terminal methods are offered, nothing else, and no MCP servers.
    let caps = rig.agent.client_caps();
    assert_eq!(
        caps["fs"],
        json!({"readTextFile": true, "writeTextFile": true})
    );
    assert_eq!(caps["terminal"], json!(true));
    assert!(caps.get("elicitation").is_none());
    let new = rig.agent.new_session();
    assert_eq!(new["cwd"], json!(CWD));
    assert_eq!(new["mcpServers"], json!([]));
    assert_eq!(rig.spawned.plans()[0].cwd, abs(CWD));

    let events = run_turn(&mut rig, "tidy the project").await;

    // Only the person's words went in the prompt.
    let prompts = rig.agent.prompts();
    assert_eq!(prompts.len(), 1);
    assert_eq!(
        prompts[0]["prompt"],
        json!([{"type": "text", "text": "tidy the project"}])
    );

    // The reads: inside the directory served, outside refused with no content.
    assert_eq!(
        rig.agent.reply("read_in").unwrap()["content"],
        json!("# readme\n")
    );
    assert!(rig.agent.reply("read_out").is_err());
    assert_eq!(files.reads(), 1, "the file outside was never opened");

    // The writes: inside only, and the person was asked.
    assert!(rig.agent.reply("write_in").is_ok());
    assert_eq!(
        files.text(&abs("/work/app/out.txt")).as_deref(),
        Some("hello")
    );
    assert!(rig.agent.reply("write_out").is_err());
    assert!(files.text(&abs("/work/other/x.txt")).is_none());
    assert_eq!(rig.performer.undo_notes().len(), 1);
    assert_eq!(rig.performer.undo_notes()[0].before, None);

    // Permission: the person's Always became a grant in docket's store, and the agent got
    // allow_once, twice (the second without a question).
    assert_eq!(
        selected(&rig.agent.reply("perm1")).as_deref(),
        Some("a-once")
    );
    assert_eq!(
        selected(&rig.agent.reply("perm2")).as_deref(),
        Some("a-once")
    );
    let held = rig.grants();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].caller, GrantCaller::AcpAgent(program()));
    assert!(matches!(
        &held[0].scope,
        StandingScope::Files { action, under }
            if action.name.as_str() == "acpagent.edit" && under.as_str() == CWD
    ));
    assert_eq!(rig.sheets().len(), 3, "write, first permission, command");

    // The command ran in the sandbox, after a question, in the session's directory.
    let ran = rig.sandbox.started();
    assert_eq!(ran.len(), 1);
    assert_eq!(ran[0].argv.line(), "echo hi");
    assert_eq!(ran[0].cwd.as_str(), CWD);
    assert_eq!(rig.agent.reply("out").unwrap()["output"], json!("hi\n"));
    assert_eq!(rig.agent.reply("exit").unwrap()["exitCode"], json!(0));
    assert!(rig.agent.reply("release").is_ok());

    // What the host sees: each call announced, then ended, words, and the end.
    let shapes: Vec<&str> = events
        .iter()
        .map(|e| match e {
            BackendEvent::Call(CallEvent::Started(_)) => "start",
            BackendEvent::Call(CallEvent::Ended(_)) => "end",
            BackendEvent::Words(_) => "words",
            BackendEvent::TurnEnd(TurnEnd::Done) => "done",
            _ => "other",
        })
        .collect();
    // read, read, write, write, terminal create: five calls. Permission requests are not shown.
    let mut want: Vec<&str> = ["start", "end"].repeat(5);
    want.extend(["words", "done"]);
    assert_eq!(shapes, want);

    // The router audited every call that reached it, as the agent program, and the grant.
    let agent = Actor::Acp {
        program: prov::AgentProgram::parse("claude-code").expect("program"),
        label: None,
    };
    let calls: Vec<String> = rig
        .audit()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Call { actor, action, .. } if actor == agent => {
                Some(action.name.as_str().to_owned())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        calls,
        [
            FILES_READ,
            FILES_WRITE,
            "acpagent.edit",
            "acpagent.edit",
            TERMINAL_RUN
        ]
    );
    assert!(
        rig.audit()
            .iter()
            .any(|r| matches!(r, AuditRecord::StandingGranted { caller, .. } if *caller == GrantCaller::AcpAgent(program())))
    );
    assert_eq!(
        rig.host.backend().tainted_by(),
        Some(&TaintSource::Served(abs("/work/app/README.md"))),
        "serving a file taints the session, and says which file"
    );
}

#[tokio::test]
async fn the_host_takes_a_second_turn_and_a_closed_one_takes_none() {
    let (mut rig, _files) = started(Setup {
        turns: vec![
            vec![say("one"), Act::Stop("end_turn")],
            vec![say("two"), Act::Stop("end_turn")],
        ],
        ..Setup::default()
    })
    .await;
    let first = run_turn(&mut rig, "first").await;
    assert!(matches!(
        first.last(),
        Some(BackendEvent::TurnEnd(TurnEnd::Done))
    ));
    let second = run_turn(&mut rig, "second").await;
    assert!(matches!(
        second.last(),
        Some(BackendEvent::TurnEnd(TurnEnd::Done))
    ));
    assert_eq!(rig.agent.prompts().len(), 2);
    let session = rig.session.clone();
    rig.host
        .close(&session, EndCause::Closed)
        .await
        .expect("close");
    assert_eq!(
        rig.spawned.closed(),
        1,
        "the process and what it was lent are given back"
    );
    assert_eq!(rig.spawned.killed(), 1);
    assert!(
        rig.host
            .turn(&session, super::rig::turn(3, "x"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn an_agent_that_exits_ends_the_turn_failed() {
    let (mut rig, _files) = started(Setup {
        turns: vec![vec![say("starting"), Act::Exit]],
        ..Setup::default()
    })
    .await;
    // The agent ends without answering the prompt: the pipe closes.
    let events = run_turn(&mut rig, "go").await;
    assert!(
        matches!(events.last(), Some(BackendEvent::TurnEnd(TurnEnd::Failed))),
        "{events:?}"
    );
}

#[tokio::test]
async fn a_wait_for_exit_ends_when_the_command_is_killed() {
    let tid = |r: &super::agent::Replies| r["term"].clone().unwrap()["terminalId"].clone();
    let (mut rig, _files) = started(Setup {
        turns: vec![vec![
            call(
                "term",
                "terminal/create",
                json!({"sessionId": AGENT_SESSION, "command": "sleep", "args": ["1000"]}),
            ),
            // The wait is outstanding while the agent kills the command.
            fire_with(
                "wait",
                "terminal/wait_for_exit",
                move |r| json!({"sessionId": AGENT_SESSION, "terminalId": tid(r)}),
            ),
            call_with(
                "kill",
                "terminal/kill",
                move |r| json!({"sessionId": AGENT_SESSION, "terminalId": tid(r)}),
            ),
            Act::Collect("wait"),
            Act::Stop("end_turn"),
        ]],
        answers: vec![once()],
        scripts: vec![Script::hangs("")],
        ..Setup::default()
    })
    .await;
    run_turn(&mut rig, "go").await;
    assert!(rig.agent.reply("kill").is_ok());
    assert_eq!(rig.agent.reply("wait").unwrap()["signal"], json!("KILL"));
}

#[tokio::test]
async fn a_yes_to_a_permission_request_covers_the_one_matching_write_and_no_more() {
    let (mut rig, files) = started(Setup {
        turns: vec![vec![
            // Serving a file taints the session, so what follows asks.
            call("r", "fs/read_text_file", read("/work/app/README.md")),
            call(
                "perm",
                "session/request_permission",
                tool("edit", "Edit a", &["/work/app/a.rs"], json!({})),
            ),
            call("w1", "fs/write_text_file", write("/work/app/a.rs", "one")),
            call("w2", "fs/write_text_file", write("/work/app/a.rs", "two")),
            Act::Stop("end_turn"),
        ]],
        // The permission request is answered once; the matching write runs on that yes without a
        // second question; the second write is asked and refused.
        answers: vec![once(), no()],
        ..Setup::default()
    })
    .await;
    files.put(&abs("/work/app/README.md"), "# readme\n");
    run_turn(&mut rig, "go").await;
    assert!(rig.agent.reply("w1").is_ok());
    assert!(rig.agent.reply("w2").is_err());
    assert_eq!(files.text(&abs("/work/app/a.rs")).as_deref(), Some("one"));
    assert_eq!(rig.sheets().len(), 2);
    assert!(
        rig.audit()
            .iter()
            .any(|r| matches!(r, AuditRecord::ApprovalUsed { .. }))
    );
}

#[tokio::test]
async fn a_resume_starts_a_new_agent_session_and_keeps_the_taint() {
    use docket_acp::client::fake::FakeSpawn;
    use docket_acp::client::{AcpBackend, Parts};
    use docket_session::{ResumePlan, Resumed, SessionBackend, Standing, Taint};
    let (rig, _files) = started(Setup {
        turns: vec![vec![Act::Stop("end_turn")]],
        ..Setup::default()
    })
    .await;
    let (wire, _view) = super::agent::agent(vec![vec![Act::Stop("end_turn")]]);
    // A second spawn for the resume: a fresh backend whose spawner holds one wire.
    let (spawn, seen) = FakeSpawn::new(vec![wire]);
    let mut backend = AcpBackend::<super::rig::Fakes>::new(Parts {
        program: super::rig::program(),
        session: rig.session.clone(),
        spawn,
        performer: rig.performer.clone(),
        court: rig.court.clone(),
        tools: None,
    });
    let plan = ResumePlan {
        opening: super::rig::opening(CWD),
        standing: Standing::Open,
        taint: Taint::Tainted,
        policy: None,
        turns: Vec::new(),
        history: Vec::new(),
        interrupted: Vec::new(),
        handles: Vec::new(),
        budget: docket_session::ResumedBudget {
            checkpoint: None,
            calls_since: porter_core::Count(0),
        },
        skills: Vec::new(),
        legacy_skipped: porter_core::Count(0),
        faults: Vec::new(),
    };
    assert_eq!(backend.resume(&plan).await, Ok(Resumed::Reseeded));
    assert!(backend.tainted(), "a resume never launders taint");
    assert_eq!(backend.tainted_by(), Some(&TaintSource::Resumed));
    assert_eq!(seen.plans().len(), 1);
    backend.close().await;
}

fn meta() -> (docket_acp::client::SessionMeta, serde_json::Value) {
    let value = json!({"claudeCode": {"options": {"settings": {"permissions": {"allow": ["mcp__quire"]}}}}});
    let serde_json::Value::Object(map) = value.clone() else {
        panic!("object")
    };
    (map, value)
}

#[tokio::test]
async fn session_new_carries_the_launchers_meta_and_nothing_else_adds_one() {
    let (map, value) = meta();
    let (rig, _files) = started(Setup {
        turns: vec![vec![Act::Stop("end_turn")]],
        session_meta: Some(map),
        ..Setup::default()
    })
    .await;
    // The host wrote it into its own request; the agent only receives it.
    assert_eq!(rig.agent.new_session()["_meta"], value);
}

#[tokio::test]
async fn session_new_has_no_meta_without_one() {
    let (rig, _files) = started(Setup {
        turns: vec![vec![Act::Stop("end_turn")]],
        ..Setup::default()
    })
    .await;
    assert!(rig.agent.new_session().get("_meta").is_none());
}

/// Whether the router counts the agent of `rig` as working, asked as the person's launcher.
async fn router_says_working(rig: &super::rig::Rig<super::rig::Fakes>) -> TurnState {
    let launcher = CallerId {
        app: AppId {
            name: app("org.quire.Shell"),
            isolation: Isolation::Unsandboxed,
        },
        roles: std::collections::BTreeSet::from([CallerRole::Launcher]),
    };
    let request = IntentsRequest::CheckpointList {
        session: rig.session.clone(),
    };
    match rig.router.handle(&launcher, request).await {
        IntentsReply::Checkpoints(list) => list.turn,
        other => panic!("list: {other:?}"),
    }
}

#[tokio::test]
async fn every_way_a_turn_ends_tells_the_router_it_is_over() {
    let rows = [
        ("answered", vec![say("done"), Act::Stop("end_turn")]),
        ("cancelled", vec![Act::Stop("cancelled")]),
        ("refused", vec![Act::Stop("refusal")]),
        ("failed", vec![say("starting"), Act::Exit]),
    ];
    for (name, acts) in rows {
        let (mut rig, _files) = started(Setup {
            turns: vec![acts],
            ..Setup::default()
        })
        .await;
        let events = run_turn(&mut rig, "go").await;
        assert!(
            matches!(events.last(), Some(BackendEvent::TurnEnd(_))),
            "{name}"
        );
        // The router heard it: no restore is held back by this turn.
        assert_eq!(router_says_working(&rig).await, TurnState::Idle, "{name}");
    }
}
