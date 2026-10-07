//! A writer's ceiling below the effect of an action it chose itself contradicts its own draft:
//! it is raised to that effect, the correction is recorded, and nothing else widens. Scripted
//! replies only (the live Qwen3.5-35B wrote `ceiling: "read"` for archive and forward).

mod support;

use docket_core::*;
use intentd::InferdWriter;
use prov::{Effect, SpaceId, TaskId, UnixSeconds};
use serde_json::json;
use support::inferd::*;

fn turn(text: &str) -> UserTurn {
    UserTurn {
        id: TurnId(1),
        text: text.into(),
        at: UnixSeconds(1),
        from: TurnSource::Launcher,
        via: TurnVia::Typed,
    }
}

fn catalogue() -> Vec<ActionCard> {
    let manifest = docket_fake::mail_manifest().expect("manifest");
    let manifest = manifest.manifest();
    manifest
        .actions
        .iter()
        .map(|a| ActionCard {
            action: ActionRef {
                app: manifest.app.clone(),
                name: a.name.clone(),
            },
            label: a.label.clone(),
            effect: a.effect,
            on: a.on.clone(),
            tool: tool_schema(a),
            reach: a.reach,
            lasting: a.lasting,
        })
        .collect()
}

fn key(name: &str) -> String {
    format!("org.quire.Mail {name}")
}

async fn derive(draft: serde_json::Value, said: &str) -> Result<Derived, ReviewError> {
    InferdWriter::new(ScriptedInferd::answering(&draft.to_string()))
        .derive(
            &TaskId::parse("t-1").expect("task"),
            &[turn(said)],
            &catalogue(),
            &SpaceId::parse("work").expect("space"),
        )
        .await
}

fn draft(actions: &[&str], apps: serde_json::Value, ceiling: &str) -> serde_json::Value {
    let actions: Vec<String> = actions.iter().map(|a| key(a)).collect();
    json!({
        "actions": actions, "apps": apps, "kinds": ["mail.thread"], "ceiling": ceiling,
        "max_count": 5, "recipients": [], "destinations": [], "paths": [],
    })
}

#[tokio::test]
async fn archive_with_a_read_ceiling_is_raised_to_undoable_write_and_says_so() {
    let derived = derive(
        draft(&["mail.thread.archive"], json!([]), "read"),
        "archive the newsletters",
    )
    .await
    .expect("policy");
    assert_eq!(derived.policy.ceiling, Effect::UndoableWrite);
    assert_eq!(
        derived.corrected,
        vec![Corrected::CeilingRaised {
            from: Effect::Read,
            to: Effect::UndoableWrite
        }]
    );
}

#[tokio::test]
async fn forward_with_a_read_ceiling_is_raised_to_outbound() {
    let derived = derive(
        draft(
            &["mail.thread.read", "mail.message.forward"],
            json!([]),
            "read",
        ),
        "forward the Lisbon receipts",
    )
    .await
    .expect("policy");
    assert_eq!(derived.policy.ceiling, Effect::Outbound);
    assert_eq!(derived.policy.actions.len(), 2, "the actions are as chosen");
}

#[tokio::test]
async fn a_ceiling_higher_than_needed_is_cut_as_before_and_is_no_correction() {
    let derived = derive(
        draft(&["mail.thread.archive"], json!([]), "destructive"),
        "archive the newsletters",
    )
    .await
    .expect("policy");
    assert_eq!(derived.policy.ceiling, Effect::UndoableWrite);
    assert!(derived.corrected.is_empty());
    let enough = derive(
        draft(&["mail.thread.archive"], json!([]), "undoable_write"),
        "archive the newsletters",
    )
    .await
    .expect("policy");
    assert!(enough.corrected.is_empty());
}

#[tokio::test]
async fn raising_adds_no_action_and_never_widens_an_app_grant() {
    let apps = json!([{ "app": "org.quire.Mail", "up_to": "read" }]);
    let derived = derive(
        draft(&["mail.thread.archive"], apps, "read"),
        "archive the newsletters",
    )
    .await
    .expect("policy");
    let app = porter_core::AppName::parse("org.quire.Mail").expect("app");
    assert_eq!(derived.policy.ceiling, Effect::UndoableWrite);
    let named = |m: &ActionMatch| matches!(m, ActionMatch::AppUpTo(_, Effect::Read));
    assert_eq!(derived.policy.actions.len(), 2);
    assert!(derived.policy.actions.iter().any(named), "grant stays read");
    assert!(
        !derived
            .policy
            .actions
            .contains(&ActionMatch::AppUpTo(app, Effect::UndoableWrite))
    );
}

#[tokio::test]
async fn an_app_grant_alone_is_not_a_floor_for_the_ceiling() {
    let apps = json!([{ "app": "org.quire.Mail", "up_to": "undoable_write" }]);
    let derived = derive(draft(&[], apps, "read"), "tidy my inbox")
        .await
        .expect("policy");
    assert_eq!(derived.policy.ceiling, Effect::Read);
    assert!(derived.corrected.is_empty());
}

#[tokio::test]
async fn a_named_action_outside_the_catalogue_is_still_dropped_and_raises_nothing() {
    let reply = json!({
        "actions": [key("mail.everything.delete")], "apps": [], "kinds": [], "ceiling": "read",
        "max_count": 5, "recipients": [], "destinations": [], "paths": [],
    });
    let derived = derive(reply, "archive the newsletters")
        .await
        .expect("policy");
    assert!(derived.policy.actions.is_empty());
    assert_eq!(derived.policy.ceiling, Effect::Read);
    assert!(derived.corrected.is_empty());
}
