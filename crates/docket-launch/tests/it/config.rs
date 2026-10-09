//! `agents.toml`: what is accepted, and what is refused outright.

use bulkhead::NetworkMode;
use docket_launch::{AgentsFile, ConfigFault, Delivery, Route};

use super::support::{ENDPOINT, HANDOFF, LOGIN};

#[test]
fn the_claude_code_entry_reads_as_written() {
    let file = AgentsFile::parse(ENDPOINT).expect("file");
    assert_eq!(file.len(), 1);
    let entry = file
        .get(&docket_session::ProgramName::parse("claude-code").expect("name"))
        .expect("entry");
    assert_eq!(entry.route, Route::Endpoint);
    assert_eq!(entry.network, NetworkMode::EndpointOnly);
    assert_eq!(
        entry.key_env.as_ref().map(|k| k.as_str()),
        Some("ANTHROPIC_API_KEY")
    );
    assert_eq!(entry.state.len(), 2);
    assert_eq!(entry.delivery, Delivery::Value);
    assert_eq!(file.programs().len(), 1);
}

#[test]
fn the_network_defaults_to_none_and_host_is_only_what_the_person_wrote() {
    let text = r#"
[[agent]]
program = "plain"
command = "/usr/bin/true"
route = "login"
"#;
    let file = AgentsFile::parse(text).expect("file");
    let entry = file.by_program(&"plain".parse_program()).expect("entry");
    assert_eq!(entry.network, NetworkMode::None);
    assert_eq!(AgentsFile::parse(LOGIN).expect("login").programs().len(), 1);
    assert!(AgentsFile::parse(HANDOFF).is_ok());
}

trait Program {
    fn parse_program(&self) -> porter_core::capability::AgentProgram;
}

impl Program for str {
    fn parse_program(&self) -> porter_core::capability::AgentProgram {
        porter_core::capability::AgentProgram::parse(self).expect("program")
    }
}

fn refused(text: &str) -> ConfigFault {
    AgentsFile::parse(text).expect_err(text)
}

fn entry(extra: &str) -> String {
    format!("[[agent]]\nprogram = \"x\"\ncommand = \"/usr/bin/x\"\n{extra}\n")
}

#[test]
fn an_entry_that_does_not_make_sense_refuses_the_whole_file() {
    // The endpoint route cannot reach its endpoint with no network.
    assert!(matches!(
        refused(&entry("route = \"endpoint\"\nnetwork = \"none\"")),
        ConfigFault::Entry { .. }
    ));
    // An endpoint route needs its pair of variables and its target.
    assert!(matches!(
        refused(&entry(
            "route = \"endpoint\"\nnetwork = \"endpoint_only\"\nkey_env = \"K\""
        )),
        ConfigFault::Entry { .. }
    ));
    assert!(matches!(
        refused(&entry(
            "route = \"endpoint\"\nnetwork = \"endpoint_only\"\nkey_env = \"K\"\nbase_url_env = \"B\""
        )),
        ConfigFault::Entry { .. }
    ));
    // A handed-off key is for a program that reaches its provider, and says so.
    assert!(matches!(
        refused(&entry("route = \"handoff\"\nkey_env = \"K\"")),
        ConfigFault::Entry { .. }
    ));
    // endpoint_only belongs to the endpoint route.
    assert!(matches!(
        refused(&entry("route = \"login\"\nnetwork = \"endpoint_only\"")),
        ConfigFault::Entry { .. }
    ));
}

#[test]
fn paths_variables_and_unknown_fields_are_checked() {
    assert!(matches!(
        refused(&entry("route = \"login\"\nstate = [\"relative/dir\"]")),
        ConfigFault::Entry { .. }
    ));
    assert!(matches!(
        refused(&entry("route = \"login\"\nreads = [\"/\"]")),
        ConfigFault::Entry { .. }
    ));
    assert!(matches!(
        refused("[[agent]]\nprogram = \"x\"\ncommand = \"bin/x\"\nroute = \"login\"\n"),
        ConfigFault::Entry { .. }
    ));
    for bad in ["LD_PRELOAD", "PATH", "HOME", "BASH_ENV", "DOCKET_X"] {
        let text = format!("{}[agent.set]\n{bad} = \"1\"\n", entry("route = \"login\""));
        assert!(matches!(refused(&text), ConfigFault::Entry { .. }), "{bad}");
    }
    let key = format!(
        "{}[agent.set]\nK = \"1\"\n",
        entry("route = \"handoff\"\nnetwork = \"host\"\nkey_env = \"K\"")
    );
    assert!(
        matches!(refused(&key), ConfigFault::Entry { .. }),
        "a key cannot be set in the clear"
    );
    assert!(matches!(
        refused(&entry("route = \"login\"\nsurprise = 1")),
        ConfigFault::Syntax(_)
    ));
    assert!(matches!(refused("not toml ["), ConfigFault::Syntax(_)));
    assert!(matches!(
        refused(
            "[[agent]]\nprogram = \"Not Lower\"\ncommand = \"/usr/bin/x\"\nroute = \"login\"\n"
        ),
        ConfigFault::Entry { .. }
    ));
}

#[test]
fn a_program_listed_twice_refuses() {
    let two = format!(
        "{}{}",
        entry("route = \"login\""),
        entry("route = \"login\"")
    );
    assert_eq!(refused(&two), ConfigFault::Twice("x".to_owned()));
}

#[test]
fn an_empty_file_lists_nothing() {
    assert!(AgentsFile::parse("").expect("empty").is_empty());
}

#[test]
fn the_shipped_example_reads_in_both_of_its_forms() {
    let text = include_str!("../../../../dist/agents.example.toml");
    let file = AgentsFile::parse(text).expect("the example");
    assert_eq!(file.len(), 2);
    let claude = file
        .by_program(&"claude-code".parse_program())
        .expect("claude");
    assert_eq!(claude.profile, Some(docket_launch::Profile::ClaudeCode));
    let agy = file.by_program(&"agy".parse_program()).expect("agy");
    assert_eq!(agy.profile, None);
    assert_eq!(
        agy.sign_in.as_ref().map(|s| s.as_str()),
        Some("oauth-personal")
    );
    assert_eq!(
        agy.label.as_ref().map(|l| l.0.as_str()),
        Some("Antigravity")
    );
    assert_eq!(agy.args, ["--uid="]);
    // The commented endpoint entry, uncommented in place of the first, reads too.
    let (_, endpoint) = text.split_once("# [[agent]]").expect("second form");
    let uncommented: String = std::iter::once("[[agent]]\n".to_owned())
        .chain(endpoint.lines().map(|l| {
            format!(
                "{}\n",
                l.strip_prefix("# ").unwrap_or(l.trim_start_matches('#'))
            )
        }))
        .collect();
    let file = AgentsFile::parse(&uncommented).expect("the endpoint form");
    assert_eq!(file.len(), 1);
}

#[test]
fn the_desktops_actions_are_offered_unless_the_entry_says_off() {
    let text = |tools: &str| {
        format!(
            "[[agent]]\nprogram = \"plain\"\ncommand = \"/usr/bin/true\"\nroute = \"login\"\n{tools}"
        )
    };
    let tools_of = |text: String| {
        AgentsFile::parse(&text)
            .map(|f| f.by_program(&"plain".parse_program()).expect("entry").tools)
    };
    assert_eq!(tools_of(text("")), Ok(docket_launch::ToolsMode::Offered));
    assert_eq!(
        tools_of(text("tools = \"off\"\n")),
        Ok(docket_launch::ToolsMode::Off)
    );
    assert!(tools_of(text("tools = \"yes\"\n")).is_err());
}

fn labelled(label: &str) -> String {
    format!(
        "[[agent]]\nprogram = \"plain\"\ncommand = \"/usr/bin/true\"\nroute = \"login\"\nlabel = {label}\n"
    )
}

fn label_of(text: &str) -> Option<String> {
    let file = AgentsFile::parse(text).expect("file");
    let entry = file.by_program(&"plain".parse_program()).expect("entry");
    entry.label.as_ref().map(|l| l.0.clone())
}

#[test]
fn a_label_is_the_trimmed_text_the_person_wrote_and_is_optional() {
    assert_eq!(
        label_of(&labelled(r#""  Claude Code ""#)),
        Some("Claude Code".to_owned())
    );
    let bare = "[[agent]]\nprogram = \"plain\"\ncommand = \"/usr/bin/true\"\nroute = \"login\"\n";
    assert_eq!(label_of(bare), None);
}

#[test]
fn a_label_that_is_empty_long_or_has_control_characters_is_refused_naming_the_program() {
    let long = format!("\"{}\"", "x".repeat(65));
    for bad in [
        r#""   ""#.to_owned(),
        long,
        r#""two\nlines""#.to_owned(),
        r#""tab\there""#.to_owned(),
    ] {
        match AgentsFile::parse(&labelled(&bad)) {
            Err(ConfigFault::Entry { program, .. }) => assert_eq!(program, "plain"),
            other => panic!("{bad}: {other:?}"),
        }
    }
    let longest = format!("\"{}\"", "x".repeat(64));
    assert!(AgentsFile::parse(&labelled(&longest)).is_ok());
}

fn signed_in(value: &str) -> String {
    format!(
        "[[agent]]\nprogram = \"plain\"\ncommand = \"/usr/bin/true\"\nroute = \"login\"\nsign_in = {value}\n"
    )
}

#[test]
fn a_sign_in_method_is_a_short_id_and_is_optional() {
    let file = AgentsFile::parse(&signed_in(r#""oauth-personal""#)).expect("file");
    let entry = file.by_program(&"plain".parse_program()).expect("entry");
    assert_eq!(
        entry.sign_in.as_ref().map(|s| s.as_str()),
        Some("oauth-personal")
    );
    let bare = "[[agent]]\nprogram = \"plain\"\ncommand = \"/usr/bin/true\"\nroute = \"login\"\n";
    let file = AgentsFile::parse(bare).expect("file");
    let entry = file.by_program(&"plain".parse_program()).expect("entry");
    assert!(entry.sign_in.is_none());
}

#[test]
fn a_sign_in_method_that_is_empty_long_or_odd_is_refused_naming_the_program() {
    let long = format!("\"{}\"", "x".repeat(65));
    for bad in [
        r#""""#.to_owned(),
        long,
        r#""two words""#.to_owned(),
        r#""new\nline""#.to_owned(),
        r#""a/b""#.to_owned(),
    ] {
        match AgentsFile::parse(&signed_in(&bad)) {
            Err(ConfigFault::Entry { program, .. }) => assert_eq!(program, "plain"),
            other => panic!("{bad}: {other:?}"),
        }
    }
}

// ---- agents from the registry ----

fn installed_agy(root: &std::path::Path) -> docket_agents::AgentsDir {
    use docket_agents::record::{Check, FILES, Record};
    let dir = docket_agents::AgentsDir::at(root);
    let slug = |t: &str| docket_agents::slug::Slug::parse(t).expect("slug");
    let at = dir.installed(&slug("agy-acp"), &slug("1.3.0"));
    std::fs::create_dir_all(at.join(FILES)).expect("dir");
    let record = Record {
        id: "agy-acp".to_owned(),
        version: "1.3.0".to_owned(),
        command: "agy".to_owned(),
        args: vec!["--uid=".to_owned()],
        env: Default::default(),
        digest: None,
        check: Check::FirstUse,
    };
    record.write(&at).expect("record");
    dir
}

const REGISTRY_ENTRY: &str = r#"
[[agent]]
program = "antigravity"
registry = "agy-acp"
version = "1.3.0"
network = "host"
sign_in = "google"
model = "gemini-pro-agent"
label = "Antigravity"
"#;

#[test]
fn a_registry_agent_runs_from_its_install_with_a_home_of_its_own() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = installed_agy(tmp.path());
    let file = AgentsFile::parse_in(REGISTRY_ENTRY, Some(&dir)).expect("file");
    let entry = file
        .get(&docket_session::ProgramName::parse("antigravity").expect("name"))
        .expect("entry");
    let root = tmp.path().to_str().expect("utf-8");
    assert_eq!(entry.route, Route::Login);
    assert_eq!(entry.args, ["--uid="]);
    assert_eq!(
        entry.command.as_str(),
        format!("{root}/installed/agy-acp/1.3.0/files/agy")
    );
    let home = format!("{root}/state/agy-acp/home");
    assert_eq!(entry.home.as_ref().map(|h| h.as_str()), Some(home.as_str()));
    assert_eq!(entry.state[0].as_str(), home);
    assert_eq!(entry.reads.len(), 1);
    assert_eq!(
        entry.model.as_ref().map(|m| m.as_str()),
        Some("gemini-pro-agent")
    );
    assert_eq!(entry.sign_in.as_ref().map(|m| m.as_str()), Some("google"));
}

#[test]
fn a_registry_agent_that_is_not_installed_or_not_pinned_is_refused() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = docket_agents::AgentsDir::at(tmp.path());
    let fault = AgentsFile::parse_in(REGISTRY_ENTRY, Some(&dir)).expect_err("not installed");
    assert!(matches!(fault, ConfigFault::Registry { .. }));
    let unpinned = REGISTRY_ENTRY.replace("version = \"1.3.0\"\n", "");
    assert!(AgentsFile::parse_in(&unpinned, Some(&dir)).is_err());
    // Without the agents directory a registry entry cannot be placed at all.
    assert!(AgentsFile::parse(REGISTRY_ENTRY).is_err());
    let both = REGISTRY_ENTRY.replace("registry =", "command = \"/usr/bin/x\"\nregistry =");
    assert!(AgentsFile::parse_in(&both, Some(&dir)).is_err());
}

#[test]
fn a_model_id_with_a_space_is_refused() {
    refused(&entry("route = \"login\"\nmodel = \"two words\""));
    let ok = entry("route = \"login\"\nmodel = \"flash\"");
    assert!(AgentsFile::parse(&ok).is_ok());
}
