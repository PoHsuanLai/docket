//! The execute taint, narrowed (acp-sessions.md section 12, R11), against the scripted fake agent
//! and the real router: after a read only a command derived from what was read loses "always";
//! a command that can send data out always asks, whatever was read and whatever is held.

use super::agent::{AGENT_SESSION, Act, call};
use super::rig::{CWD, Setup, abs, always, once, program, read, run_turn, started};
use bulkhead::{Network, NetworkMode, fake::Script};
use docket_core::{
    AlwaysOffer, AuditRecord, CommandPrefix, GrantCaller, StandingGrant, StandingScope,
    TERMINAL_RUN, Withheld, acp_agent_action,
};
use prov::UnixSeconds;
use serde_json::json;

fn run(tag: &'static str, command: &str, args: &[&str]) -> Act {
    call(
        tag,
        "terminal/create",
        json!({"sessionId": AGENT_SESSION, "command": command, "args": args}),
    )
}

fn grant(prefix: &str) -> StandingGrant {
    StandingGrant::new(
        GrantCaller::AcpAgent(program()),
        StandingScope::Terminal {
            action: acp_agent_action(TERMINAL_RUN).expect("action"),
            command: CommandPrefix::parse(prefix).expect("prefix"),
            cwd: abs(CWD),
        },
        UnixSeconds(1),
    )
}

fn read_then(rest: Vec<Act>) -> Vec<Act> {
    let mut acts = vec![call("r", "fs/read_text_file", read("/work/app/a.txt"))];
    acts.extend(rest);
    acts.push(Act::Stop("end_turn"));
    acts
}

fn used(rig: &super::rig::Rig<super::rig::Fakes>) -> usize {
    rig.audit()
        .iter()
        .filter(|r| matches!(r, AuditRecord::StandingUsed { .. }))
        .count()
}

#[tokio::test]
async fn after_a_read_cargo_test_is_granted_always_and_the_second_run_asks_nothing() {
    let (mut rig, files) = started(Setup {
        turns: vec![read_then(vec![
            run("one", "cargo", &["test"]),
            run("two", "cargo", &["test", "--lib"]),
        ])],
        answers: vec![always()],
        scripts: vec![Script::done("ok\n", 0), Script::done("ok\n", 0)],
        ..Setup::default()
    })
    .await;
    files.put(
        &abs("/work/app/a.txt"),
        "fn main() { let key = \"sk_live_Ab12Cd34\"; }",
    );
    run_turn(&mut rig, "go").await;
    assert!(rig.agent.reply("one").is_ok() && rig.agent.reply("two").is_ok());
    assert_eq!(rig.sheets().len(), 1, "the second run asked nothing");
    assert!(matches!(
        rig.sheets()[0].always,
        AlwaysOffer::Offered(StandingScope::Terminal { .. })
    ));
    assert_eq!(rig.grants().len(), 1);
    assert_eq!(used(&rig), 1);
    assert_eq!(rig.sandbox.started().len(), 2);
}

#[tokio::test]
async fn a_command_using_a_token_from_the_file_asks_with_no_always_even_with_a_grant() {
    let (mut rig, files) = started(Setup {
        turns: vec![read_then(vec![
            run("tok", "echo", &["sk_live_Ab12Cd34"]),
            run("path", "echo", &["/work/app/a.txt"]),
            run("rel", "echo", &["./a.txt"]),
        ])],
        answers: vec![once(), once(), once()],
        scripts: vec![
            Script::done("", 0),
            Script::done("", 0),
            Script::done("", 0),
        ],
        held: vec![grant("echo")],
        ..Setup::default()
    })
    .await;
    files.put(
        &abs("/work/app/a.txt"),
        "fn main() { let key = \"sk_live_Ab12Cd34\"; }",
    );
    run_turn(&mut rig, "go").await;
    let sheets = rig.sheets();
    assert_eq!(sheets.len(), 3, "the grant stood in for none of them");
    for sheet in sheets {
        assert_eq!(
            sheet.always,
            AlwaysOffer::Withheld(Withheld::UntrustedIntoSink)
        );
    }
    assert_eq!(used(&rig), 0);
    assert_eq!(rig.grants().len(), 1, "no new grant");
}

#[tokio::test]
async fn curl_always_asks_with_no_reads_and_a_matching_grant() {
    let (mut rig, _files) = started(Setup {
        turns: vec![vec![
            run("curl", "curl", &["https://a.test/x"]),
            run("push", "git", &["push", "origin", "main"]),
            Act::Stop("end_turn"),
        ]],
        answers: vec![once(), once()],
        scripts: vec![Script::done("", 0), Script::done("", 0)],
        held: vec![grant("curl"), grant("git push")],
        ..Setup::default()
    })
    .await;
    run_turn(&mut rig, "go").await;
    let sheets = rig.sheets();
    assert_eq!(sheets.len(), 2);
    for sheet in sheets {
        assert_eq!(sheet.always, AlwaysOffer::Withheld(Withheld::CanSendOut));
    }
    assert_eq!(used(&rig), 0);
}

#[tokio::test]
async fn an_agent_with_a_network_makes_every_command_one_that_can_send_data_out() {
    let (mut rig, _files) = started(Setup {
        turns: vec![vec![run("t", "cargo", &["test"]), Act::Stop("end_turn")]],
        answers: vec![once()],
        scripts: vec![Script::done("", 0)],
        held: vec![grant("cargo test")],
        network: Network::Host,
        ..Setup::default()
    })
    .await;
    run_turn(&mut rig, "go").await;
    assert_eq!(rig.sheets().len(), 1, "the grant did not stand in");
    assert_eq!(
        rig.sheets()[0].always,
        AlwaysOffer::Withheld(Withheld::CanSendOut)
    );
    assert_eq!(rig.sandbox.started()[0].network, Network::Host);
}

#[tokio::test]
async fn shell_operators_are_still_never_covered_by_a_grant() {
    let (mut rig, _files) = started(Setup {
        turns: vec![vec![
            run("semi", "cargo", &["test;", "echo", "done"]),
            run("and", "cargo", &["test", "&&", "echo", "done"]),
            Act::Stop("end_turn"),
        ]],
        answers: vec![once(), once()],
        scripts: vec![Script::done("", 0), Script::done("", 0)],
        held: vec![grant("cargo test")],
        ..Setup::default()
    })
    .await;
    run_turn(&mut rig, "go").await;
    assert_eq!(rig.sheets().len(), 2);
    assert_eq!(used(&rig), 0);
}

#[tokio::test]
async fn what_the_agents_own_tool_brought_in_makes_every_command_derived() {
    let claim = json!({
        "sessionUpdate": "tool_call", "toolCallId": "t9", "title": "Grep", "kind": "search",
        "status": "completed", "rawInput": {}, "rawOutput": {}
    });
    let (mut rig, _files) = started(Setup {
        turns: vec![vec![
            Act::Update(claim),
            run("t", "cargo", &["test"]),
            Act::Stop("end_turn"),
        ]],
        answers: vec![once()],
        scripts: vec![Script::done("", 0)],
        held: vec![grant("cargo test")],
        ..Setup::default()
    })
    .await;
    run_turn(&mut rig, "go").await;
    assert_eq!(rig.sheets().len(), 1);
    assert_eq!(
        rig.sheets()[0].always,
        AlwaysOffer::Withheld(Withheld::UntrustedIntoSink)
    );
}

#[tokio::test]
async fn an_agent_process_with_a_network_makes_every_command_one_that_can_send_data_out() {
    for mode in [NetworkMode::EndpointOnly, NetworkMode::Host] {
        let (mut rig, _files) = started(Setup {
            turns: vec![read_then(vec![run("t", "cargo", &["test"])])],
            answers: vec![once()],
            scripts: vec![Script::done("", 0)],
            held: vec![grant("cargo test")],
            agent_network: mode,
            ..Setup::default()
        })
        .await;
        run_turn(&mut rig, "go").await;
        let sheets = rig.sheets();
        assert_eq!(sheets.len(), 1, "{mode:?}: the grant did not stand in");
        assert_eq!(
            sheets[0].always,
            AlwaysOffer::Withheld(Withheld::CanSendOut),
            "{mode:?}"
        );
        assert_eq!(used(&rig), 0);
        assert_eq!(
            rig.sandbox.started()[0].network,
            Network::Off,
            "the command's own sandbox is unchanged"
        );
    }
}

#[tokio::test]
async fn an_agent_process_without_a_network_leaves_the_rule_as_it_was() {
    let (mut rig, files) = started(Setup {
        turns: vec![read_then(vec![run("t", "cargo", &["test"])])],
        answers: vec![always()],
        scripts: vec![Script::done("ok\n", 0)],
        agent_network: NetworkMode::None,
        ..Setup::default()
    })
    .await;
    files.put(&abs("/work/app/a.txt"), "plain words only");
    run_turn(&mut rig, "go").await;
    let sheets = rig.sheets();
    assert_eq!(sheets.len(), 1);
    assert!(matches!(sheets[0].always, AlwaysOffer::Offered(_)));
    assert_eq!(rig.grants().len(), 1, "the always was granted");
}
