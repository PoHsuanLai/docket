//! Only offered actions become tools, names and hints follow the manifest, and every argument
//! a client sends is untrusted.

use actions_mcp::*;
use docket_core::*;
use porter_core::AppName;
use prov::{ClientName, Effect, Integrity, Source};
use std::path::PathBuf;

fn manifest(file: &str) -> ValidManifest {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../manifests")
        .join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    validate(toml::from_str::<Manifest>(&text).expect("manifest")).expect("valid")
}

#[test]
fn tools_only_offered_actions() {
    let registry = [
        manifest("org.quire.Memory.toml"),
        manifest("org.quire.Companion.toml"),
    ];
    let names: Vec<String> = offered(&registry)
        .into_iter()
        .map(|(_, a)| a.name.to_string())
        .collect();
    assert!(names.contains(&"memory.recall".to_owned()));
    assert!(
        !names.contains(&"memory.forget".to_owned()),
        "a hidden action is never a tool"
    );
    assert_eq!(
        names.len(),
        3 + 3,
        "three offered memory actions and the three companion actions"
    );
}

#[test]
fn tool_names_are_the_app_slug_and_the_action_with_underscores() {
    let memory = manifest("org.quire.Memory.toml");
    let recall = memory
        .manifest()
        .actions
        .iter()
        .find(|a| a.name.as_str() == "memory.recall")
        .expect("recall");
    let name = tool_name(&memory.manifest().app, recall).expect("name");
    assert_eq!(name.as_str(), "memory__memory_recall");
    assert!(
        name.as_str()
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    );
}

#[test]
fn a_name_over_sixty_four_characters_has_no_tool() {
    let memory = manifest("org.quire.Memory.toml");
    let mut action = memory.manifest().actions[0].clone();
    action.name = prov::ActionName::parse(&format!("memory.{}.{}", "a".repeat(30), "b".repeat(30)))
        .expect("action");
    assert_eq!(
        tool_name(&memory.manifest().app, &action),
        Err(McpFault::TooLong)
    );
}

#[test]
fn tool_hints_follow_effect() {
    let cases = [
        (Effect::Read, ToolHints::ReadOnly),
        (Effect::UndoableWrite, ToolHints::Undoable),
        (Effect::Outbound, ToolHints::OpenWorld),
        (Effect::Destructive, ToolHints::Destructive),
    ];
    for (effect, want) in cases {
        assert_eq!(hints_of(effect), want, "{effect:?}");
    }
}

#[test]
fn every_argument_from_a_client_is_untrusted_and_says_whose() {
    let client = ClientName::parse("Claude Desktop").expect("client");
    let label = mcp_label(&client);
    assert_eq!(label.integrity, Integrity::Untrusted);
    assert_eq!(
        label.sources.iter().collect::<Vec<_>>(),
        [&Source::Mcp(client)]
    );
}

#[test]
fn the_registry_becomes_tools_with_schemas() {
    let registry = [manifest("org.quire.Memory.toml")];
    let tools = tools(&registry);
    assert_eq!(tools.len(), 3);
    assert!(tools.iter().all(|t| t.schema.0.get("type").is_some()));
    let _ = AppName::parse("org.quire.Memory");
}
