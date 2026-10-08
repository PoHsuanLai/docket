//! A whole session with the fake agent: initialize, session/new, a prompt in which the agent
//! reads and writes files, asks permission, runs a command, and ends.

use super::agent::{AGENT_SESSION, Act, call, call_with, fire_with, say};
use super::rig::{CWD, Setup, abs, read, run_turn, selected, started, tool, write};
use docket_acp::Answer;
use docket_acp::client::Audit;
use docket_session::{BackendEvent, CallEvent, SessionBackend, TurnEnd};
use docket_shell::fake::Script;
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
async fn a_full_session_goes_through_the_gate_call_by_call() {
    let mut rig = started(Setup {
        turns: vec![script()],
        // write_in: once. perm1: always (a standing grant). The command: once.
        answers: vec![Answer::Once, Answer::Always, Answer::Once],
        scripts: vec![Script::done("hi\n", 0)],
        ..Setup::default()
    })
    .await;
    rig.files.put(&abs("/work/app/README.md"), "# readme\n");
    rig.files.put(&abs("/etc/passwd"), "root:x:0:0");

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

    let events = run_turn(&mut rig.backend, "tidy the project").await;

    // Only the person's words went in the prompt.
    let prompts = rig.agent.prompts();
    assert_eq!(prompts.len(), 1);
    assert_eq!(
        prompts[0]["prompt"],
        json!([{"type": "text", "text": "tidy the project"}])
    );

    // The reads: inside the directory served, outside refused with no content.
    assert!(rig.agent.reply("read_in").is_ok());
    assert_eq!(
        rig.agent.reply("read_in").unwrap()["content"],
        json!("# readme\n")
    );
    assert!(rig.agent.reply("read_out").is_err());
    assert_eq!(rig.files.reads(), 1, "the file outside was never opened");

    // The writes: inside only, and the person was asked.
    assert!(rig.agent.reply("write_in").is_ok());
    assert_eq!(
        rig.files.text(&abs("/work/app/out.txt")).as_deref(),
        Some("hello")
    );
    assert!(rig.agent.reply("write_out").is_err());
    assert!(rig.files.text(&abs("/work/other/x.txt")).is_none());
    assert_eq!(rig.backend.undo_notes().len(), 1);
    assert_eq!(rig.backend.undo_notes()[0].before, None);

    // Permission: the person's Always became our grant, and the agent got allow_once, twice
    // (the second without a question).
    assert_eq!(
        selected(&rig.agent.reply("perm1")).as_deref(),
        Some("a-once")
    );
    assert_eq!(
        selected(&rig.agent.reply("perm2")).as_deref(),
        Some("a-once")
    );
    assert_eq!(rig.backend.grants().len(), 1);
    let questions = rig.ask.questions();
    assert_eq!(
        questions.len(),
        3,
        "write, first permission, command: {questions:?}"
    );

    // The command ran in the sandbox, after a question, in the session's directory.
    let ran = rig.sandbox.started();
    assert_eq!(ran.len(), 1);
    assert_eq!(ran[0].argv.line(), "echo hi");
    assert_eq!(ran[0].cwd, abs(CWD));
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
    // read, read, write, write, terminal create: five calls. Permission requests are not calls.
    let mut want: Vec<&str> = ["start", "end"].repeat(5);
    want.extend(["words", "done"]);
    assert_eq!(shapes, want);

    // The audit: a grant was stored; nothing says the agent's own option decided anything.
    let audit = rig.backend.take_audit();
    assert!(audit.iter().any(|a| matches!(a, Audit::GrantStored(_))));
    assert!(rig.backend.tainted(), "serving a file taints the session");
}

#[tokio::test]
async fn the_backend_takes_a_second_turn_and_a_closed_one_takes_none() {
    let mut rig = started(Setup {
        turns: vec![
            vec![say("one"), Act::Stop("end_turn")],
            vec![say("two"), Act::Stop("end_turn")],
        ],
        ..Setup::default()
    })
    .await;
    let first = run_turn(&mut rig.backend, "first").await;
    assert!(matches!(
        first.last(),
        Some(BackendEvent::TurnEnd(TurnEnd::Done))
    ));
    let second = run_turn(&mut rig.backend, "second").await;
    assert!(matches!(
        second.last(),
        Some(BackendEvent::TurnEnd(TurnEnd::Done))
    ));
    assert_eq!(rig.agent.prompts().len(), 2);
    rig.backend.close().await;
    assert_eq!(
        rig.spawned.closed(),
        1,
        "the process and what it was lent are given back"
    );
    assert_eq!(rig.spawned.killed(), 1);
    assert!(rig.backend.turn(super::rig::turn(3, "x")).await.is_err());
}

#[tokio::test]
async fn an_agent_that_exits_ends_the_turn_failed() {
    let mut rig = started(Setup {
        turns: vec![vec![say("starting"), Act::Exit]],
        ..Setup::default()
    })
    .await;
    // The agent ends without answering the prompt: the pipe closes.
    let events = run_turn(&mut rig.backend, "go").await;
    assert!(
        matches!(events.last(), Some(BackendEvent::TurnEnd(TurnEnd::Failed))),
        "{events:?}"
    );
}

#[tokio::test]
async fn a_wait_for_exit_ends_when_the_command_is_killed() {
    let tid = |r: &super::agent::Replies| r["term"].clone().unwrap()["terminalId"].clone();
    let mut rig = started(Setup {
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
        answers: vec![Answer::Once],
        scripts: vec![Script::hangs("")],
        ..Setup::default()
    })
    .await;
    run_turn(&mut rig.backend, "go").await;
    assert!(rig.agent.reply("kill").is_ok());
    assert_eq!(rig.agent.reply("wait").unwrap()["signal"], json!("KILL"));
}

#[tokio::test]
async fn a_yes_to_a_permission_request_covers_the_one_matching_write_and_no_more() {
    let mut rig = started(Setup {
        turns: vec![vec![
            call(
                "perm",
                "session/request_permission",
                tool("edit", "Edit a", &["/work/app/a.rs"], json!({})),
            ),
            call("w1", "fs/write_text_file", write("/work/app/a.rs", "one")),
            call("w2", "fs/write_text_file", write("/work/app/a.rs", "two")),
            Act::Stop("end_turn"),
        ]],
        // The permission request is answered once; the second write is asked and refused.
        answers: vec![Answer::Once, Answer::No],
        ..Setup::default()
    })
    .await;
    run_turn(&mut rig.backend, "go").await;
    assert!(rig.agent.reply("w1").is_ok());
    assert!(rig.agent.reply("w2").is_err());
    assert_eq!(
        rig.files.text(&abs("/work/app/a.rs")).as_deref(),
        Some("one")
    );
    assert_eq!(rig.ask.questions().len(), 2);
}

#[tokio::test]
async fn a_resume_starts_a_new_agent_session_and_keeps_the_taint() {
    use docket_session::{ResumePlan, Resumed, Standing, Taint};
    let mut rig = started(Setup {
        turns: vec![vec![Act::Stop("end_turn")]],
        ..Setup::default()
    })
    .await;
    let (wire, _view) = super::agent::agent(vec![vec![Act::Stop("end_turn")]]);
    // A second spawn for the resume: build a fresh backend whose spawner holds one wire.
    let (spawn, seen) = docket_acp::client::fake::FakeSpawn::new(vec![wire]);
    let (sandbox, _) = docket_shell::fake::FakeSandbox::ready(Vec::new());
    let mut backend =
        docket_acp::client::AcpBackend::<super::rig::Fakes>::new(docket_acp::client::Parts {
            program: super::rig::program(),
            session: super::rig::session(),
            spawn,
            files: docket_acp::client::fake::FakeFiles::new(),
            ask: docket_acp::client::fake::FakeAsk::new(Vec::new()),
            sandbox,
            ticks: crate::support::Fixed,
            grants: Vec::new(),
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
    assert_eq!(seen.plans().len(), 1);
    rig.backend.close().await;
}
