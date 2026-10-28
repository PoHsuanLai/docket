//! One tool list and one tool call made into a request, for every edge: the menu narrows both,
//! an argument that does not fit is refused, and a call carries the client's label.

use actions_tools::{ToolFault, listing, prepare, tools};
use docket_manifest::{ArgsFault, Manifest, ValidManifest, validate};
use docket_vocab::Origin;
use prov::{ClientName, Integrity};
use serde_json::{Value as Json, json};
use std::path::PathBuf;

fn manifest(file: &str) -> ValidManifest {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../manifests")
        .join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    validate(toml::from_str::<Manifest>(&text).expect("manifest")).expect("valid")
}

fn client() -> ClientName {
    ClientName::parse("Claude Desktop").expect("client")
}

fn memory() -> [ValidManifest; 1] {
    [manifest("org.quire.Memory.toml")]
}

#[test]
fn the_listing_is_the_tools_of_the_registry_with_their_labels() {
    let registry = memory();
    let listed: Vec<_> = listing(&registry, |_| true)
        .into_iter()
        .map(|(tool, _)| tool)
        .collect();
    assert_eq!(listed, tools(&registry));
}

#[test]
fn the_menu_narrows_the_listing() {
    let registry = memory();
    assert!(listing(&registry, |_| false).is_empty());
    let only_recall = listing(&registry, |a| a.name.as_str() == "memory.recall");
    assert_eq!(only_recall.len(), 1);
    assert_eq!(only_recall[0].0.name.as_str(), "memory__memory_recall");
}

#[test]
fn a_tool_off_the_menu_is_unknown() {
    let registry = memory();
    assert_eq!(
        prepare(
            &registry,
            "memory__memory_recall",
            &Json::Null,
            &client(),
            |_| false
        ),
        Err(ToolFault::UnknownTool)
    );
    assert_eq!(
        prepare(&registry, "nothing__here", &Json::Null, &client(), |_| true),
        Err(ToolFault::UnknownTool)
    );
}

#[test]
fn arguments_that_are_not_an_object_are_refused() {
    let registry = memory();
    assert_eq!(
        prepare(
            &registry,
            "memory__memory_recall",
            &json!([1]),
            &client(),
            |_| true
        ),
        Err(ArgsFault::NotAnObject.into())
    );
}

#[test]
fn a_prepared_call_names_the_action_and_comes_from_a_client() {
    let registry = memory();
    let call = prepare(
        &registry,
        "memory__memory_recall",
        &json!({ "query": "tea" }),
        &client(),
        |_| true,
    )
    .expect("prepared");
    assert_eq!(call.action.name.as_str(), "memory.recall");
    assert_eq!(call.origin, Origin::Mcp);
    assert!(
        call.args
            .values()
            .all(|held| held.label.integrity == Integrity::Untrusted),
        "every argument from a client is untrusted"
    );
}
