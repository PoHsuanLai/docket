//! The ACP engine's command line, the agent's `agents.toml` entry, an agent from the registry and the
//! settings a world reads back. No daemon, no sandbox, no network.

use crate::support::words;
use docket_accept::world::{AcpSetting, settings_toml};
use docket_core::AgentConfig;
use docket_live::acp::{AcpSpec, Task};
use docket_live::cli::{Agent, Command, UsageError, parse};
use docket_settings::{AgentSettings, read};
use std::path::PathBuf;

#[test]
fn an_acp_run_names_its_agent_on_the_command_line() {
    let line = "smoke --engine scripted --agent acp --acp-command /home/u/.local/bin/claude-agent-acp \
                --acp-arg --verbose --acp-state .claude --acp-state .claude.json \
                --acp-reads /home/u/.local/share/node --acp-credentials /home/u/creds.json \
                --acp-set FOO=bar --acp-profile claude-code --acp-sign-in oauth-personal \
                --flow flow-a";
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
    assert_eq!(spec.profile.as_deref(), Some("claude-code"));
    assert_eq!(spec.sign_in.as_deref(), Some("oauth-personal"));
    assert!(
        spec.entry_toml(std::path::Path::new("/home/u/scratch"))
            .contains("sign_in = \"oauth-personal\"\n")
    );
    assert!(
        spec.entry_toml(std::path::Path::new("/home/u/scratch"))
            .contains("profile = \"claude-code\"\n")
    );
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
        "smoke --engine scripted --agent acp --acp-command /x --acp-profile bogus",
        "smoke --engine scripted --agent acp --acp-command /x --acp-profile agy",
        "smoke --engine scripted --agent acp --acp-command /x --acp-sign-in a/b",
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
        .get(&docket_permissions_core::ProgramName::parse("claude-code").expect("name"))
        .expect("entry");
    assert_eq!(entry.route, docket_launch::Route::Login);
    assert_eq!(entry.network, bulkhead::NetworkMode::Host);
    assert_eq!(entry.state[0].as_str(), "/scratch/world-1/.claude");
    assert_eq!(
        entry.home.as_ref().map(|h| h.as_str()),
        Some("/scratch/world-1")
    );
    assert_eq!(entry.tools, docket_launch::ToolsMode::Offered);
    assert_eq!(entry.set[0].1, "a \"quoted\" value");
}

fn acp_spec(line: &str) -> Result<AcpSpec, UsageError> {
    match parse(&words(line))? {
        Command::Smoke(s) => match s.agent {
            Agent::Acp(spec) => Ok(*spec),
            Agent::Planner => panic!("planner"),
        },
        Command::Corpus(_) => panic!("smoke"),
    }
}

#[test]
fn a_registry_agent_needs_its_pin_and_directory_and_takes_no_login_of_its_own() {
    let base = "smoke --engine scripted --agent acp --acp-agent agy-acp";
    assert!(acp_spec(base).is_err());
    assert!(acp_spec(&format!("{base} --acp-version 1.3.0")).is_err());
    let line = format!("{base} --acp-version 1.3.0 --acp-agents-dir /scratch/agents");
    let spec = acp_spec(&format!("{line} --acp-model gemini-pro-agent")).expect("spec");
    assert_eq!(spec.program, "agy-acp");
    assert_eq!(spec.model.as_deref(), Some("gemini-pro-agent"));
    assert_eq!(spec.task, Task::Flows);
    assert!(acp_spec(&format!("{line} --acp-credentials /x")).is_err());
    assert!(acp_spec(&format!("{line} --acp-command /x")).is_err());
    let list = acp_spec(&format!("{line} --acp-list-models")).expect("list");
    assert_eq!(list.task, Task::ListModels);
}

#[test]
fn a_registry_entry_names_the_install_and_leaves_the_home_to_launch() {
    let line = "smoke --engine scripted --agent acp --acp-agent agy-acp --acp-version 1.3.0 \
        --acp-agents-dir /scratch/agents --acp-model gemini-pro-agent --acp-sign-in google";
    let text = acp_spec(line)
        .expect("spec")
        .entry_toml(std::path::Path::new("/s"));
    assert!(text.contains("registry = \"agy-acp\""));
    assert!(text.contains("version = \"1.3.0\""));
    assert!(text.contains("model = \"gemini-pro-agent\""));
    assert!(!text.contains("command ="));
    assert!(!text.contains("home ="));
    assert!(!text.contains("state ="));
}

/// Every world's settings read back without a fallback or an unknown key, whatever the ACP setting.
#[test]
fn a_worlds_settings_read_back_clean() {
    for acp in [AcpSetting::Off, AcpSetting::On, AcpSetting::Agents] {
        let loaded = read(
            &settings_toml(acp),
            AgentSettings::over(AgentConfig::default()),
        );
        assert!(
            loaded.fallbacks.is_empty(),
            "{acp:?}: {:?}",
            loaded.fallbacks
        );
        assert!(loaded.unknown.is_empty(), "{acp:?}: {:?}", loaded.unknown);
    }
}
