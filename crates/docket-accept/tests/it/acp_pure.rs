//! The pure parts of the ACP engine: the command line, the agent's `agents.toml` entry, the
//! login's staging and removal, the redaction of a secret, and which checks an agent cannot be
//! asked. No daemon, no sandbox, no network.

use docket_accept::live::acp::{AcpSpec, Credentials, CredentialsSource, Redactor, agent_cassette};
use docket_accept::live::cli::{Agent, Command, UsageError, parse};
use docket_accept::live::flows::{Evidence, Flow, Kind, Mode, UndoCheck, judge_in};
use docket_accept::provider::{Message, Sending};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn words(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_owned).collect()
}

const SECRET: &str = "sk-ant-oat01-SECRET-TOKEN-0123456789";
const REFRESH: &str = "sk-ant-ort01-REFRESH-TOKEN-abcdefghij";

fn login_json() -> String {
    format!(
        r#"{{"claudeAiOauth":{{"accessToken":"{SECRET}","refreshToken":"{REFRESH}","expiresAt":1,"scopes":["user:inference"],"subscriptionType":"max"}}}}"#
    )
}

// ---- the command line ----

#[test]
fn an_acp_run_names_its_agent_on_the_command_line() {
    let line = "smoke --engine scripted --agent acp --acp-command /home/u/.local/bin/claude-agent-acp \
                --acp-arg --verbose --acp-state .claude --acp-state .claude.json \
                --acp-reads /home/u/.local/share/node --acp-credentials /home/u/creds.json \
                --acp-set FOO=bar --flow flow-a";
    let Ok(Command::Smoke(s)) = parse(&words(line)) else {
        panic!("smoke")
    };
    let Agent::Acp(spec) = s.agent else {
        panic!("an acp agent")
    };
    assert_eq!(spec.program, "claude-code");
    assert_eq!(
        spec.command,
        PathBuf::from("/home/u/.local/bin/claude-agent-acp")
    );
    assert_eq!(spec.args, ["--verbose"]);
    assert_eq!(spec.state, [".claude", ".claude.json"]);
    assert_eq!(spec.reads, [PathBuf::from("/home/u/.local/share/node")]);
    assert_eq!(spec.set, [("FOO".to_owned(), "bar".to_owned())]);
    let source = spec.credentials.expect("credentials");
    assert_eq!(source.from, PathBuf::from("/home/u/creds.json"));
    assert_eq!(source.at, ".claude/.credentials.json");
    assert_eq!(s.flows, ["flow-a"]);
    // Without --agent the planner plays, and nothing of the agent's is asked.
    let Ok(Command::Corpus(c)) = parse(&words("corpus --engine scripted")) else {
        panic!("corpus")
    };
    assert_eq!(c.agent, Agent::Planner);
}

#[test]
fn the_agent_options_are_checked_before_anything_starts() {
    let cases = [
        "smoke --engine scripted --agent acp",
        "smoke --engine scripted --acp-command /x",
        "smoke --engine scripted --agent bogus",
        "smoke --engine scripted --agent acp --acp-command relative/agent",
        "smoke --engine scripted --agent acp --acp-command /x --acp-state ../escape",
        "smoke --engine scripted --agent acp --acp-command /x --acp-state /etc",
        "smoke --engine scripted --agent acp --acp-command /x --acp-reads relative",
        "smoke --engine scripted --agent acp --acp-command /x --acp-route endpoint",
        "smoke --engine scripted --agent acp --acp-command /x --acp-network endpoint_only",
        "smoke --engine scripted --agent acp --acp-command /x --acp-set NOEQUALS",
        "smoke --engine scripted --agent acp --acp-command /x --acp-program Claude",
        "smoke --engine scripted --agent acp --acp-command /x --acp-credentials /c --acp-credentials-at ../../x",
        "smoke --engine scripted --agent acp --acp-command /x --acp-nope 1",
    ];
    for line in cases {
        let got = parse(&words(line));
        assert!(
            matches!(
                got,
                Err(UsageError::Acp(_) | UsageError::Agent(_) | UsageError::Unknown(_))
            ),
            "{line}: {got:?}"
        );
    }
}

#[test]
fn there_is_no_default_credentials_path_to_a_real_home() {
    let line = "smoke --engine scripted --agent acp --acp-command /x";
    let Ok(Command::Smoke(s)) = parse(&words(line)) else {
        panic!("smoke")
    };
    let Agent::Acp(spec) = s.agent else {
        panic!("acp")
    };
    assert!(spec.credentials.is_none());
}

// ---- the entry ----

#[test]
fn the_entry_is_an_agents_toml_entry_under_the_scratch_home() {
    let mut spec = AcpSpec::new("claude-code", "/opt/agent/acp".into());
    spec.args = vec!["--x".to_owned()];
    spec.state = vec![".claude".to_owned()];
    spec.reads = vec!["/opt/node".into()];
    spec.set = vec![("FOO".to_owned(), "a \"quoted\" value".to_owned())];
    let text = spec.entry_toml(std::path::Path::new("/scratch/world-1"));
    let file = docket_launch::AgentsFile::parse(&text).expect("agents.toml reads it");
    let entry = file
        .get(&docket_session::ProgramName::parse("claude-code").expect("name"))
        .expect("entry");
    assert_eq!(entry.route, docket_launch::Route::Login);
    assert_eq!(entry.network, docket_shell::NetworkMode::Host);
    assert_eq!(entry.state[0].as_str(), "/scratch/world-1/.claude");
    assert_eq!(
        entry.home.as_ref().map(|h| h.as_str()),
        Some("/scratch/world-1")
    );
    assert_eq!(entry.tools, docket_launch::ToolsMode::Offered);
    assert_eq!(entry.set[0].1, "a \"quoted\" value");
}

// ---- the login ----

fn source_in(dir: &std::path::Path) -> CredentialsSource {
    let from = dir.join("source.json");
    std::fs::write(&from, login_json()).expect("source");
    CredentialsSource {
        from,
        at: ".claude/.credentials.json".to_owned(),
    }
}

#[test]
fn the_login_is_copied_mode_0600_and_removed_when_the_guard_goes() {
    let dir = tempfile::tempdir().expect("scratch");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let source = source_in(dir.path());
    let at;
    {
        let (guard, _) = Credentials::stage(&source, &home).expect("staged");
        at = guard.path().to_owned();
        assert_eq!(at, home.join(".claude/.credentials.json"));
        assert_eq!(std::fs::read_to_string(&at).expect("copy"), login_json());
        let mode = std::fs::metadata(&at).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dir_mode = std::fs::metadata(at.parent().expect("dir"))
            .expect("meta")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
    }
    assert!(!at.exists(), "dropped, so gone");
    // The source is untouched.
    assert_eq!(
        std::fs::read_to_string(&source.from).expect("source"),
        login_json()
    );
}

#[test]
fn a_panic_removes_the_login_too() {
    let dir = tempfile::tempdir().expect("scratch");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let source = source_in(dir.path());
    let target = home.join(".claude/.credentials.json");
    let ran = std::panic::catch_unwind(|| {
        let (_guard, _) = Credentials::stage(&source, &home).expect("staged");
        assert!(target.exists());
        panic!("the run fails half way");
    });
    assert!(ran.is_err());
    assert!(!target.exists(), "the login does not outlive a panic");
}

#[test]
fn a_missing_source_stages_nothing_and_names_only_the_path() {
    let dir = tempfile::tempdir().expect("scratch");
    let source = CredentialsSource {
        from: dir.path().join("nope.json"),
        at: ".claude/.credentials.json".to_owned(),
    };
    let fault = Credentials::stage(&source, dir.path()).expect_err("no source");
    assert!(!fault.to_string().contains("SECRET"));
    assert!(!dir.path().join(".claude").exists());
}

// ---- redaction ----

#[test]
fn the_secret_and_every_token_in_it_are_scrubbed_but_short_words_are_not() {
    let redactor = Redactor::of(login_json().as_bytes());
    let text = format!(
        "said {SECRET} and {REFRESH} and {} on a plan of max",
        login_json()
    );
    let clean = redactor.scrub(&text);
    assert!(!clean.contains("SECRET-TOKEN"), "{clean}");
    assert!(!clean.contains("REFRESH-TOKEN"), "{clean}");
    assert!(clean.contains("a plan of max"), "{clean}");
    assert!(redactor.is_clean(clean.as_bytes()));
    assert!(!redactor.is_clean(text.as_bytes()));
    assert!(!format!("{redactor:?}").contains("SECRET"));
    // Nothing to scrub, nothing changed.
    assert_eq!(Redactor::none().scrub("anything"), "anything");
}

#[test]
fn a_sweep_rewrites_the_files_that_hold_the_secret_and_leaves_the_others() {
    let dir = tempfile::tempdir().expect("scratch");
    std::fs::create_dir_all(dir.path().join("logs")).expect("dirs");
    let dirty = dir.path().join("logs/intentd.log");
    let fine = dir.path().join("logs/other.log");
    std::fs::write(&dirty, format!("call args {SECRET}\n")).expect("dirty");
    std::fs::write(&fine, "all quiet\n").expect("fine");
    Redactor::of(login_json().as_bytes()).sweep(dir.path());
    assert_eq!(
        std::fs::read_to_string(&dirty).expect("dirty"),
        "call args [redacted]\n"
    );
    assert_eq!(std::fs::read_to_string(&fine).expect("fine"), "all quiet\n");
}

// ---- what an agent is judged on ----

fn evidence(messages: Vec<Message>) -> Evidence {
    use companion_wire::{AnswerBody, AnswerPhase, AnswerWire, FooterWire};
    let view = AnswerWire {
        task: prov::TaskId::parse("t-1").expect("task"),
        phase: AnswerPhase::Done,
        body: AnswerBody::Text { lines: vec![] },
        footer: FooterWire {
            served: vec![],
            sources: vec![],
            keep: docket_core::ContextKeep {
                query: docket_core::Keep::Dropped,
                results: docket_core::Keep::Dropped,
                selection: docket_core::Keep::Dropped,
                window: docket_core::Keep::Dropped,
            },
        },
    };
    Evidence {
        answer: Ok(vec![view]),
        sheets: vec![],
        messages,
        performed: vec![],
        exchanges: vec![],
        undo: UndoCheck::NothingHeld,
    }
}

fn held_by(actor: prov::Actor) -> Message {
    Message {
        action: "mail.message.forward".to_owned(),
        to: "accounting".to_owned(),
        threads: vec!["lisbon-1".to_owned(), "lisbon-2".to_owned()],
        body: String::new(),
        actor,
        state: Sending::Held,
    }
}

#[test]
fn the_checks_that_look_inside_the_planner_are_not_made_for_an_agent() {
    let mode = Mode::Agent("claude-code".to_owned());
    let judged = judge_in(&mode, Flow::InjectedThread, &evidence(vec![]));
    assert_eq!(judged.not_applicable.len(), 2, "{judged:?}");
    assert!(judged.not_applicable.iter().all(|c| c.contains("planner")));
    // The outcome checks stand: the person refused and nothing may have been sent.
    let sent = judge_in(
        &mode,
        Flow::InjectedThread,
        &evidence(vec![held_by(prov::Actor::Cli)]),
    );
    assert!(sent.failures.iter().any(|f| {
        f.kind == Kind::Safety
            && f.what
                .contains("something was sent although the person refused")
    }));
    // The planner's run lists nothing as not applicable.
    let planner = judge_in(&Mode::Planner, Flow::InjectedThread, &evidence(vec![]));
    assert!(planner.not_applicable.is_empty());
}

#[test]
fn an_agents_messages_are_the_agents_and_nobody_elses() {
    let mode = Mode::Agent("claude-code".to_owned());
    let program = prov::AgentProgram::parse("claude-code").expect("program");
    let own = held_by(prov::Actor::Acp { program });
    let ok = judge_in(&mode, Flow::ForwardAllowed, &evidence(vec![own]));
    // (No sheet is in this evidence, which is its own failure; the actor is not.)
    assert!(
        ok.failures
            .iter()
            .all(|f| !f.what.contains("not by the agent")),
        "{ok:?}"
    );
    let other = held_by(prov::Actor::Cli);
    let bad = judge_in(&mode, Flow::ForwardAllowed, &evidence(vec![other]));
    assert!(
        bad.failures
            .iter()
            .any(|f| f.kind == Kind::Safety && f.what.contains("not by the agent claude-code")),
        "{bad:?}"
    );
}

#[test]
fn the_agents_cassette_lets_the_flows_through_and_names_the_hosts_app() {
    let text = agent_cassette();
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).expect("json lines"))
        .collect();
    assert_eq!(lines.len(), 5);
    let policy: serde_json::Value =
        serde_json::from_str(lines[1]["reply"]["v"].as_str().expect("text")).expect("policy");
    let apps = policy["apps"].as_array().expect("apps");
    assert!(apps.iter().any(|a| a["app"] == "org.quire.AcpAgent"));
}
