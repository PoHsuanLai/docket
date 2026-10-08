//! The launcher's duties, against the fake porter and the recording process seam: what is
//! asked of porter and in what order, what the child is given and not given, what is undone on
//! close, on a failure, and when a credential is revoked.

use super::support::{ENDPOINT, HANDOFF, HANDOFF_FILE, LOGIN, plan, rig};
use docket_acp::client::{AgentChild, Spawn, SpawnFault};
use docket_launch::fake::{Call, FakeAccounts, Mood};
use docket_launch::{Heard, Supervisor};
use docket_shell::{Access, AgentNet, AgentRun, NetworkMode, agent_bwrap_args};
use porter_core::ProcessCredentialId;
use std::os::unix::fs::PermissionsExt;

fn env_of<'a>(run: &'a AgentRun, name: &str) -> Option<&'a str> {
    run.env
        .iter()
        .find(|v| v.name == name)
        .map(|v| v.value.as_str())
}

fn names(run: &AgentRun) -> Vec<&str> {
    let mut all: Vec<&str> = run.env.iter().map(|v| v.name.as_str()).collect();
    all.sort_unstable();
    all
}

fn args_text(run: &AgentRun) -> String {
    agent_bwrap_args(run, &["/home", "/tmp"]).join(" ")
}

#[tokio::test]
async fn the_endpoint_route_gives_the_child_a_base_url_and_a_token_and_nothing_else() {
    let mut rig = rig(ENDPOINT, Mood::Working, true);
    let spawned = rig
        .spawn
        .spawn(&plan("claude-code", "s-1"))
        .await
        .expect("spawned");
    let mut child = spawned.child;

    // What porter was asked, in order: a session, then an endpoint.
    assert_eq!(
        rig.accounts.calls(),
        [
            Call::Begin("acp-s-1".into()),
            Call::Open("claude-code".into(), "anthropic-main".into())
        ]
    );
    let runs = rig.procs.runs();
    assert_eq!(runs.len(), 1);
    let run = &runs[0];

    // The environment was built from nothing: the fixed base, the entry's plain variable, and
    // the endpoint pair. Nothing of the launcher's own.
    assert_eq!(
        names(run),
        [
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            "HOME",
            "LANG",
            "NODE_NO_WARNINGS",
            "PATH",
            "TERM",
            "TMPDIR"
        ]
    );
    assert_eq!(
        env_of(run, "ANTHROPIC_BASE_URL"),
        Some("http://127.0.0.1:41999")
    );
    assert_eq!(
        env_of(run, "ANTHROPIC_API_KEY"),
        Some(rig.secrets.token.as_str())
    );
    assert_eq!(env_of(run, "HOME"), Some("/home/me"));
    // The real key is nowhere, and the token is on no command line.
    assert!(!format!("{run:?}").contains(&rig.secrets.key));
    assert!(!args_text(run).contains(&rig.secrets.token));

    // The sandbox: the session's directory, the program's directory and install read-only, its
    // own state read-write, and the network is the endpoint and nothing else.
    assert_eq!(run.cwd.as_str(), "/work/app");
    let bound = |path: &str, access: Access| {
        run.binds
            .iter()
            .any(|b| b.path.as_str() == path && b.access == access)
    };
    assert!(bound("/home/me/.local/bin", Access::ReadOnly));
    assert!(bound("/home/me/.local/share/node", Access::ReadOnly));
    assert!(bound("/home/me/.claude", Access::ReadWrite));
    assert!(bound("/home/me/.claude.json", Access::ReadWrite));
    let AgentNet::Endpoint(bind) = &run.net else {
        panic!("{:?}", run.net)
    };
    assert_eq!(bind.port, 41999);
    assert_eq!(bind.forwarder.as_str(), "/opt/docket/docket-net-forward");

    // The bridge's socket lives in a directory only the user can enter, for as long as the
    // session does.
    let dir = rig.dir.path().join("docket-agent-acp-s-1");
    assert!(dir.join("ep.sock").exists());
    assert_eq!(
        std::fs::metadata(&dir).expect("dir").permissions().mode() & 0o777,
        0o700
    );

    // Close gives everything back, in the reverse order.
    child.close().await;
    assert_eq!(
        rig.accounts.calls()[2..],
        [Call::Close("ep-1".into()), Call::End("acp-s-1".into())]
    );
    assert!(rig.procs.killed(0));
    assert!(!dir.exists(), "the socket directory goes with the session");
}

#[tokio::test]
async fn a_handed_off_key_goes_to_the_child_only_and_is_revoked_with_it() {
    let mut rig = rig(HANDOFF, Mood::Working, true);
    let spawned = rig
        .spawn
        .spawn(&plan("gemini-cli", "s-2"))
        .await
        .expect("spawned");
    let mut child = spawned.child;
    assert_eq!(
        rig.accounts.calls(),
        [
            Call::Begin("acp-s-2".into()),
            Call::Grant("gemini-cli".into()),
            Call::Issue("gemini-cli".into(), docket_launch::Delivery::Value)
        ]
    );
    let run = &rig.procs.runs()[0];
    assert_eq!(
        env_of(run, "GEMINI_API_KEY"),
        Some(rig.secrets.key.as_str())
    );
    assert_eq!(run.net, AgentNet::Host);
    assert!(
        !args_text(run).contains(&rig.secrets.key),
        "never on a command line"
    );
    assert_eq!(rig.registry.len(), 1, "a revocation can find this process");

    child.close().await;
    assert_eq!(
        rig.accounts.calls()[3..],
        [Call::Revoke("cred-1".into()), Call::End("acp-s-2".into())]
    );
    assert!(rig.registry.is_empty());
}

#[tokio::test]
async fn a_file_handoff_puts_the_key_in_no_environment() {
    let mut rig = rig(HANDOFF_FILE, Mood::Working, true);
    let _spawned = rig
        .spawn
        .spawn(&plan("gemini-cli", "s-3"))
        .await
        .expect("spawned");
    let run = &rig.procs.runs()[0];
    assert_eq!(env_of(run, "GEMINI_API_KEY"), None);
    assert_eq!(
        env_of(run, "GEMINI_API_KEY_FILE"),
        Some(rig.secrets.key_file.as_str())
    );
    assert!(!format!("{run:?}").contains(&rig.secrets.key));
    assert!(
        run.binds
            .iter()
            .any(|b| b.path.as_str() == rig.secrets.key_file && b.access == Access::ReadOnly)
    );
}

#[tokio::test]
async fn a_subscription_login_gets_no_key_at_all_and_host_network_only_because_it_was_written() {
    let mut rig = rig(LOGIN, Mood::Working, true);
    let _spawned = rig
        .spawn
        .spawn(&plan("claude-code", "s-4"))
        .await
        .expect("spawned");
    assert_eq!(rig.accounts.calls(), [Call::Begin("acp-s-4".into())]);
    let run = &rig.procs.runs()[0];
    assert_eq!(names(run), ["HOME", "LANG", "PATH", "TERM", "TMPDIR"]);
    assert_eq!(run.net, AgentNet::Host);
    assert!(args_text(run).contains("--share-net"));
    // The same entry with no network line is the default: none.
    let quiet = LOGIN.replace("network = \"host\"\n", "");
    let mut rig = rig_with(&quiet);
    let _spawned = rig
        .spawn
        .spawn(&plan("claude-code", "s-5"))
        .await
        .expect("spawned");
    assert_eq!(rig.procs.runs()[0].net, AgentNet::None);
    assert_eq!(NetworkMode::default(), NetworkMode::None);
}

fn rig_with(toml: &str) -> super::support::Rig {
    rig(toml, Mood::Working, true)
}

#[tokio::test]
async fn a_program_that_is_not_listed_starts_nothing_and_asks_nothing() {
    let mut rig = rig(ENDPOINT, Mood::Working, true);
    let err = rig
        .spawn
        .spawn(&plan("gemini-cli", "s-1"))
        .await
        .expect_err("not listed");
    assert_eq!(err, SpawnFault::NotAllowed);
    assert!(rig.accounts.calls().is_empty());
    assert!(rig.procs.runs().is_empty());
}

#[test]
fn with_the_setting_off_there_is_no_spawner_to_build() {
    for text in [
        "",
        "[agent.acp]\nagents = \"off\"\n",
        "[agent.acp]\nexpose = \"on\"\n",
    ] {
        assert!(
            docket_launch::AgentsPermit::from_text(text).is_err(),
            "{text:?}"
        );
    }
}

#[tokio::test]
async fn a_failure_part_way_undoes_what_was_done() {
    // inferd will not open the endpoint: the session is ended and nothing runs.
    let mut rig = rig(ENDPOINT, Mood::NoEndpoint, true);
    assert_eq!(
        rig.spawn
            .spawn(&plan("claude-code", "s-1"))
            .await
            .expect_err("refused"),
        SpawnFault::Accounts
    );
    assert_eq!(
        rig.accounts.calls(),
        [
            Call::Begin("acp-s-1".into()),
            Call::Open("claude-code".into(), "anthropic-main".into()),
            Call::End("acp-s-1".into())
        ]
    );
    assert!(rig.procs.runs().is_empty());

    // accountd is not there: no process, and the failed begin is all that was tried.
    let mut rig = rig_down();
    assert_eq!(
        rig.spawn
            .spawn(&plan("claude-code", "s-1"))
            .await
            .expect_err("down"),
        SpawnFault::Accounts
    );
    assert_eq!(rig.accounts.calls(), [Call::Begin("acp-s-1".into())]);

    // The process will not start after the endpoint opened: the endpoint is closed, the session
    // ended, the socket directory gone.
    let mut rig = super::support::rig(ENDPOINT, Mood::Working, false);
    assert!(rig.spawn.spawn(&plan("claude-code", "s-1")).await.is_err());
    assert_eq!(
        rig.accounts.calls()[2..],
        [Call::Close("ep-1".into()), Call::End("acp-s-1".into())]
    );
    assert!(!rig.dir.path().join("docket-agent-acp-s-1").exists());
}

fn rig_down() -> super::support::Rig {
    rig(ENDPOINT, Mood::Down, true)
}

#[tokio::test]
async fn a_revocation_ends_the_process_that_holds_the_credential() {
    let mut rig = rig(HANDOFF, Mood::Working, true);
    let _spawned = rig
        .spawn
        .spawn(&plan("gemini-cli", "s-1"))
        .await
        .expect("spawned");
    assert!(!rig.procs.killed(0));
    let accounts = std::sync::Arc::new(rig.accounts.clone());
    let supervisor = Supervisor::new(
        accounts,
        rig.file.clone(),
        rig.registry.clone(),
        super::login::Scripted::ready(),
    );
    // A revocation for another credential changes nothing; ours kills.
    rig.accounts.say(Heard::Revoked(
        ProcessCredentialId::parse("cred-9").expect("id"),
    ));
    rig.accounts.say(Heard::Revoked(
        ProcessCredentialId::parse("cred-1").expect("id"),
    ));
    assert!(supervisor.step().await);
    assert!(!rig.procs.killed(0));
    assert!(supervisor.step().await);
    assert!(
        rig.procs.killed(0),
        "accountd ended the credential: the process ends"
    );
    assert!(
        !supervisor.step().await,
        "and the link ending ends the loop"
    );
}

#[tokio::test]
async fn a_child_dropped_without_close_is_cleaned_up_all_the_same() {
    let mut rig = rig(ENDPOINT, Mood::Working, true);
    let spawned = rig
        .spawn
        .spawn(&plan("claude-code", "s-1"))
        .await
        .expect("spawned");
    drop(spawned);
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert!(rig.procs.killed(0));
    assert_eq!(
        rig.accounts.calls()[2..],
        [Call::Close("ep-1".into()), Call::End("acp-s-1".into())]
    );
}

#[tokio::test]
async fn a_long_session_id_is_mapped_to_the_launcher_grammar() {
    let mut rig = rig(ENDPOINT, Mood::Working, true);
    let long = format!("s-{}", "x".repeat(62));
    let _spawned = rig
        .spawn
        .spawn(&plan("claude-code", &long))
        .await
        .expect("spawned");
    let Call::Begin(name) = rig.accounts.calls()[0].clone() else {
        panic!()
    };
    assert_eq!(name.len(), 20);
    assert!(name.starts_with("acp-"));
    let _ = FakeAccounts::new();
}
