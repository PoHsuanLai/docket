//! `agents.toml`: what is accepted, and what is refused outright.

use docket_launch::{AgentsFile, ConfigFault, Delivery, Route};
use docket_shell::NetworkMode;

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
    assert_eq!(file.len(), 1);
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
