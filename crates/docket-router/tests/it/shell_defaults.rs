//! The shipped default consent (`dist/intents/default-grants.json`): the companion may use the
//! shell's own window and menu state without a first-use question, and nothing else. The fake
//! menu app stands in for the shell here, with its manifest renamed to `org.quire.Shell` and its
//! data class to `app_own`, so `menu.item.activate` is `shell.menu.activate`'s twin.

use crate::support::*;
use docket_core::*;
use docket_fake::{FakeMenu, MENU_MANIFEST, MenuItem, ScriptedConfirmer};
use docket_router::{GrantStore, Router, parse};
use porter_core::consent::{Decision, Grant, GrantScope};
use prov::{ActionName, AgentRef, Effect};

const SHIPPED: &str = include_str!("../../../../dist/intents/default-grants.json");

/// The shipped file as the daemon reads it: dated the epoch.
fn shipped() -> Vec<ActionGrant> {
    serde_json::from_str(SHIPPED).expect("the shipped defaults parse")
}

fn router_of(
    owner: &str,
    class: &str,
    strictness: Strictness,
    defaults: Vec<ActionGrant>,
) -> Router<docket_fake::FakeSeams> {
    let prefix = owner.rsplit('.').next().unwrap_or(owner).to_lowercase();
    let text = MENU_MANIFEST
        .replace("org.quire.Menu", owner)
        .replace("\"menu.", &format!("\"{prefix}."))
        .replace("classes = [\"files\"]", &format!("classes = [\"{class}\"]"));
    let manifest = parse(&text).expect("manifest");
    let config = AgentConfig {
        strictness,
        ..AgentConfig::default()
    };
    let mut router = docket_fake::fake_router(config).expect("router");
    router.seams.link.menu = FakeMenu::new(manifest.clone());
    router
        .state
        .lock()
        .expect("state")
        .registry
        .insert(manifest);
    router.seams.grants = docket_fake::MemoryGrants::with(defaults);
    router.seams.confirmer = ScriptedConfirmer::answering(vec![
        ConfirmAnswer::Allowed {
            scope: GrantScope::Once,
            receipt: receipt(),
        };
        4
    ]);
    router
}

fn call(owner: &str, name: &str, item: Option<&str>) -> CallRequest {
    let prefix = owner.rsplit('.').next().unwrap_or(owner).to_lowercase();
    CallRequest {
        action: ActionRef {
            app: app(owner),
            name: ActionName::parse(&format!("{prefix}.{name}")).expect("action"),
        },
        target: TargetValue::Nothing,
        args: item
            .map(|i| {
                (
                    param("item"),
                    prov::Labelled {
                        value: Value::Text(i.into()),
                        label: trusted(),
                    },
                )
            })
            .into_iter()
            .collect(),
        origin: Origin::Companion,
    }
}

fn asks(router: &Router<docket_fake::FakeSeams>) -> usize {
    router.seams.confirmer.requests().len()
}

/// Runs `request` as the companion in a fresh session and says how many questions it asked.
async fn asked(router: &Router<docket_fake::FakeSeams>, request: CallRequest) -> usize {
    let s = open(router, "work", AgentRef::Companion).await;
    say(router, &s.session, "tidy the windows").await;
    let before = asks(router);
    let _ = perform(router, request).await;
    asks(router) - before
}

const SHELL: &str = "org.quire.Shell";

fn minimise(router: &Router<docket_fake::FakeSeams>) -> CallRequest {
    router
        .seams
        .link
        .menu
        .script("Minimise", MenuItem::effect(Effect::Read));
    call(SHELL, "item.activate", Some("Minimise"))
}

#[test]
fn the_shipped_file_is_exactly_the_companion_and_the_shell_for_app_own_in_both_usages() {
    let all = shipped();
    assert_eq!(all.len(), 2);
    for grant in &all {
        assert_eq!(grant.key.caller, GrantCaller::Companion);
        assert_eq!(grant.key.owner, app(SHELL));
        assert_eq!(grant.key.target, GrantTarget::App);
        assert_eq!(grant.key.class, porter_core::DataClass::AppOwn);
        assert_eq!(grant.key.space, prov::SpaceScope::Any);
        assert_eq!(grant.decision, Decision::Allow);
        assert_eq!(grant.scope, GrantScope::Always);
    }
    let usages: std::collections::BTreeSet<_> = all.iter().map(|g| g.key.usage).collect();
    assert_eq!(usages.len(), 2, "interactive and background");
}

#[tokio::test]
async fn a_read_row_of_the_shell_does_not_ask_on_first_use_with_the_defaults() {
    let router = router_of(SHELL, "app_own", Strictness::Default, shipped());
    let request = minimise(&router);
    assert_eq!(asked(&router, request).await, 0);
}

#[tokio::test]
async fn the_same_row_asks_without_the_defaults() {
    let router = router_of(SHELL, "app_own", Strictness::Default, Vec::new());
    let request = minimise(&router);
    assert_eq!(asked(&router, request).await, 1);
}

#[tokio::test]
async fn a_revoke_the_person_recorded_wins_over_the_default() {
    let router = router_of(SHELL, "app_own", Strictness::Default, shipped());
    for (n, default) in shipped().iter().enumerate() {
        router.seams.grants.record(Grant {
            id: porter_core::GrantId::parse(&format!("g-revoke-{n}")).expect("id"),
            key: default.key.clone(),
            decision: Decision::Deny,
            scope: GrantScope::Always,
            at: prov::UnixSeconds(5),
        });
    }
    let request = minimise(&router);
    let s = open(&router, "work", AgentRef::Companion).await;
    say(&router, &s.session, "tidy the windows").await;
    let result = perform(&router, request).await;
    assert!(result.is_err(), "a revoked default refuses: {result:?}");
    assert_eq!(asks(&router), 0, "and it does not ask either");
}

#[tokio::test]
async fn another_apps_app_own_still_asks() {
    let router = router_of("dev.notes", "app_own", Strictness::Default, shipped());
    router
        .seams
        .link
        .menu
        .script("Minimise", MenuItem::effect(Effect::Read));
    let request = call("dev.notes", "item.activate", Some("Minimise"));
    assert_eq!(asked(&router, request).await, 1);
}

#[tokio::test]
async fn the_shells_other_classes_still_ask() {
    let router = router_of(SHELL, "screen", Strictness::Default, shipped());
    let request = minimise(&router);
    assert_eq!(asked(&router, request).await, 1);
}

#[tokio::test]
async fn strictness_still_rules_the_shells_writes_and_destructive_acts() {
    // A read is final in every strictness once consent is there.
    let router = router_of(SHELL, "app_own", Strictness::AskMore, shipped());
    let request = minimise(&router);
    assert_eq!(asked(&router, request).await, 0);
    // An undoable write with no task policy: judged by default, asked under AskMore.
    for (strictness, expected) in [(Strictness::Default, 0), (Strictness::AskMore, 1)] {
        let router = router_of(SHELL, "app_own", strictness, shipped());
        router
            .seams
            .link
            .menu
            .script("Move", MenuItem::effect(Effect::UndoableWrite));
        let request = call(SHELL, "item.activate", Some("Move"));
        assert_eq!(asked(&router, request).await, expected, "{strictness:?}");
    }
    // A destructive act asks even under TrustMore.
    let router = router_of(SHELL, "app_own", Strictness::TrustMore, shipped());
    router
        .seams
        .link
        .menu
        .script("Delete", MenuItem::effect(Effect::Destructive));
    let request = call(SHELL, "item.activate", Some("Delete"));
    assert_eq!(asked(&router, request).await, 1);
}
