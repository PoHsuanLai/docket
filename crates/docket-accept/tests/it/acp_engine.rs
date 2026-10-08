//! The ACP engine end to end, with a fake agent: the real intentd, inferd (replaying a cassette),
//! memoryd and the mail provider on a private bus, the agent in the real bubblewrap sandbox with
//! no network, its MCP server the real `actions-mcp` bridge over the per-session socket, and the
//! calls it makes the host's router calls. No real agent, no real login, no network; HOME and
//! every XDG directory are scratch. A run skips, with a printed reason, where bubblewrap or user
//! namespaces are not available.

use crate::support::binaries;
use docket_accept::live::acp::{AcpSpec, CredentialsSource, agent_cassette, run_flow_acp};
use docket_accept::live::flows::Flow;
use docket_accept::world::ModelSource;
use docket_shell::{Detected, NetworkMode};
use std::path::Path;
use std::time::Duration;

const SECRET: &str = "sk-ant-oat01-SECRET-TOKEN-0123456789";

fn sandbox_here() -> bool {
    match Detected::probe("/usr/bin:/bin:/usr/local/bin") {
        Detected::Bwrap(_) => true,
        Detected::Missing(why) => {
            eprintln!("SKIP acp engine test: no sandbox here ({why:?})");
            false
        }
    }
}

fn agent(script: &str) -> AcpSpec {
    let mut spec = AcpSpec::new(
        "claude-code",
        env!("CARGO_BIN_EXE_accept-fake-agent").into(),
    );
    spec.args = vec![script.to_owned()];
    // No network at all: the edge is a unix socket.
    spec.network = NetworkMode::None;
    spec
}

async fn run(
    flow: Flow,
    spec: &AcpSpec,
    scratch: &Path,
) -> docket_accept::live::acp::AcpFlowReport {
    let model = ModelSource::Scripted(agent_cassette());
    run_flow_acp(
        &binaries(),
        flow,
        spec,
        &model,
        (Some(scratch.to_owned()), None),
        Duration::from_secs(120),
    )
    .await
}

fn said(report: &docket_accept::live::acp::AcpFlowReport) -> &str {
    &report.transcript
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn flow_a_with_an_agent_forwards_to_accounting_through_the_edge() {
    if !sandbox_here() {
        return;
    }
    let scratch = tempfile::tempdir().expect("scratch");
    let report = run(Flow::ForwardAllowed, &agent("flow-a"), scratch.path()).await;
    assert!(
        report.failures.is_empty(),
        "{:?}\n{}",
        report.failures,
        said(&report)
    );
    assert!(report.not_applicable.is_empty());
    let t = said(&report);
    assert!(t.contains("the turn ended Done"), "{t}");
    // The bridge listed the actions and the three calls went through it.
    assert!(t.contains("mail__mail_message_forward: ok"), "{t}");
    assert!(
        t.contains("held: mail.message.forward to \"accounting\""),
        "{t}"
    );
    assert!(t.contains("Acp"), "the message was the agent's: {t}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refusal_stops_the_forward_and_the_agent_is_told_so() {
    if !sandbox_here() {
        return;
    }
    let scratch = tempfile::tempdir().expect("scratch");
    let report = run(Flow::ForwardRefused, &agent("flow-a"), scratch.path()).await;
    assert!(
        report.failures.is_empty(),
        "{:?}\n{}",
        report.failures,
        said(&report)
    );
    let t = said(&report);
    assert!(t.contains("mail__mail_message_forward: failed"), "{t}");
    assert!(!t.contains("held:"), "nothing was held: {t}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_first_use_of_mail_asks_and_each_search_after_it_asks_again() {
    if !sandbox_here() {
        return;
    }
    let scratch = tempfile::tempdir().expect("scratch");
    let report = run(Flow::FirstUse, &agent("first-use"), scratch.path()).await;
    assert!(
        report.failures.is_empty(),
        "{:?}\n{}",
        report.failures,
        said(&report)
    );
    // An agent holds scoped standing grants and a search has no scope: both searches asked, and
    // the report says the planner's "exactly one" was not measured.
    assert_eq!(
        report.not_applicable.len(),
        1,
        "{:?}",
        report.not_applicable
    );
    let t = said(&report);
    assert_eq!(t.matches("Find threads effect=").count(), 2, "{t}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_agent_that_obeys_the_injected_thread_is_stopped_at_the_sheet() {
    if !sandbox_here() {
        return;
    }
    let scratch = tempfile::tempdir().expect("scratch");
    let report = run(Flow::InjectedThread, &agent("flow-c"), scratch.path()).await;
    assert!(
        report.failures.is_empty(),
        "{:?}\n{}",
        report.failures,
        said(&report)
    );
    // The two checks that look inside the planner are listed, not run.
    assert_eq!(
        report.not_applicable.len(),
        2,
        "{:?}",
        report.not_applicable
    );
    let t = said(&report);
    assert!(t.contains("not applicable in this mode"), "{t}");
    assert!(t.contains("mail__mail_message_send: failed"), "{t}");
}

fn source(dir: &Path) -> CredentialsSource {
    let from = dir.join("login.json");
    std::fs::write(
        &from,
        format!(r#"{{"claudeAiOauth":{{"accessToken":"{SECRET}"}}}}"#),
    )
    .expect("login");
    CredentialsSource {
        from,
        at: ".claude/.credentials.json".to_owned(),
    }
}

fn files_under(root: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(k) if k.is_dir() => files_under(&path, out),
            Ok(k) if k.is_file() => out.push(path),
            _ => {}
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_login_reaches_the_agents_home_and_is_gone_after_the_run() {
    if !sandbox_here() {
        return;
    }
    let scratch = tempfile::tempdir().expect("scratch");
    let mut spec = agent("creds");
    spec.state = vec![".claude".to_owned()];
    spec.credentials = Some(source(scratch.path()));
    let report = run(Flow::FirstUse, &spec, scratch.path()).await;
    let t = said(&report);
    // The agent saw a file in its HOME (by length) and the report holds none of it.
    assert!(t.contains("credentials present: Some("), "{t}");
    assert!(!t.contains("SECRET-TOKEN"), "{t}");
    let mut files = Vec::new();
    files_under(scratch.path(), &mut files);
    assert!(
        files
            .iter()
            .all(|f| f.file_name().is_none_or(|n| n != ".credentials.json")),
        "the staged copy is gone: {files:?}"
    );
    for file in files
        .iter()
        .filter(|f| f.file_name().is_none_or(|n| n != "login.json"))
    {
        let bytes = std::fs::read(file).unwrap_or_default();
        assert!(
            !String::from_utf8_lossy(&bytes).contains("SECRET-TOKEN"),
            "{} holds the login",
            file.display()
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_agent_that_says_its_login_aloud_is_scrubbed_from_the_transcript_and_the_tree() {
    if !sandbox_here() {
        return;
    }
    let scratch = tempfile::tempdir().expect("scratch");
    let mut spec = agent("leak");
    spec.state = vec![".claude".to_owned()];
    spec.credentials = Some(source(scratch.path()));
    let report = run(Flow::FirstUse, &spec, scratch.path()).await;
    let t = said(&report);
    assert!(
        t.contains("my credentials are"),
        "the agent did say something: {t}"
    );
    assert!(t.contains("[redacted]"), "{t}");
    assert!(!t.contains("SECRET-TOKEN"), "{t}");
    let mut files = Vec::new();
    files_under(scratch.path(), &mut files);
    for file in files
        .iter()
        .filter(|f| f.file_name().is_none_or(|n| n != "login.json"))
    {
        let bytes = std::fs::read(file).unwrap_or_default();
        assert!(
            !String::from_utf8_lossy(&bytes).contains("SECRET-TOKEN"),
            "{} holds the login",
            file.display()
        );
    }
}

/// The packaged `docket-live` as a process, as the owner starts it: the login must not appear on
/// its stdout or stderr, in its traces or anywhere under its output directory.
#[test]
fn the_command_prints_and_writes_nothing_of_the_login() {
    if !sandbox_here() {
        return;
    }
    let scratch = tempfile::tempdir().expect("scratch");
    let home = scratch.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let login = source(scratch.path());
    let out = scratch.path().join("out");
    let catalog = scratch.path().join("catalog");
    std::fs::create_dir_all(&catalog).expect("an empty catalogue");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_docket-live"))
        .env_clear()
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/none")
        .args([
            "smoke",
            "--engine",
            "scripted",
            "--agent",
            "acp",
            "--flow",
            "first-use",
        ])
        .arg("--acp-command")
        .arg(env!("CARGO_BIN_EXE_accept-fake-agent"))
        .args([
            "--acp-arg",
            "leak",
            "--acp-network",
            "none",
            "--acp-state",
            ".claude",
        ])
        .arg("--acp-credentials")
        .arg(&login.from)
        .arg("--out")
        .arg(&out)
        .arg("--catalog")
        .arg(&catalog)
        .arg("--patience-s")
        .arg("120")
        .output()
        .expect("docket-live runs");
    let shown = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!shown.contains("SECRET-TOKEN"), "{shown}");
    assert!(
        shown.contains("copied into the scratch HOME"),
        "it says what it does: {shown}"
    );
    assert!(
        shown.contains("PASS first-use") || shown.contains("FAIL first-use"),
        "{shown}"
    );
    let mut files = Vec::new();
    files_under(&out, &mut files);
    assert!(!files.is_empty());
    for file in &files {
        let bytes = std::fs::read(file).unwrap_or_default();
        assert!(
            !String::from_utf8_lossy(&bytes).contains("SECRET-TOKEN"),
            "{} holds the login",
            file.display()
        );
    }
    assert!(
        files
            .iter()
            .all(|f| f.file_name().is_none_or(|n| n != ".credentials.json")),
        "the staged copy is gone: {files:?}"
    );
}
