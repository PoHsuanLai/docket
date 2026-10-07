//! The policy writer's draft as a hostile model might write it. Nothing in it is believed as
//! written: a recipient or a path is kept only when the person wrote it as a whole word, a count
//! and a ceiling are bounded, an action that does not exist is dropped, and a reply outside the
//! shape is no policy at all.

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

async fn derive(reply: &str, said: &str) -> Result<TaskPolicy, ReviewError> {
    InferdWriter::new(ScriptedInferd::answering(reply))
        .derive(
            &TaskId::parse("t-1").expect("task"),
            &[turn(said)],
            &catalogue(),
            &SpaceId::parse("work").expect("space"),
        )
        .await
}

fn with(extra: serde_json::Value) -> String {
    let mut base = json!({
        "actions": [], "apps": [], "kinds": [], "ceiling": "read", "max_count": 5,
        "recipients": [], "destinations": [], "paths": [],
    });
    if let (Some(base), Some(extra)) = (base.as_object_mut(), extra.as_object()) {
        base.extend(extra.clone());
    }
    base.to_string()
}

#[tokio::test]
async fn a_recipient_is_kept_only_as_a_whole_word_the_person_wrote() {
    let said = "send the receipts to alice@example.com.";
    let kept = |recipients: serde_json::Value| async move {
        derive(&with(json!({ "recipients": recipients })), said)
            .await
            .expect("policy")
            .recipients
    };
    let alice = TrustedPattern::Exact(Value::Text("alice@example.com".into()));
    let example = TrustedPattern::Domain("example.com".into());
    assert_eq!(kept(json!(["alice@example.com"])).await, [alice]);
    assert_eq!(
        kept(json!(["example.com"])).await,
        std::slice::from_ref(&example)
    );
    assert_eq!(kept(json!(["@example.com"])).await, [example]);
    assert_eq!(
        kept(json!(["ALICE@Example.com"])).await.len(),
        1,
        "case is not a name"
    );
    // Pieces of what they wrote, and what they did not write, name nothing.
    for piece in [
        "com",
        "@",
        ".",
        "a",
        "example",
        "alice",
        "example.co",
        "mple.com",
        "bob@example.com",
        "evil.test",
    ] {
        assert_eq!(kept(json!([piece])).await, [], "{piece:?}");
    }
}

#[tokio::test]
async fn a_path_is_kept_only_whole_and_never_the_root() {
    let said = "move report.pdf to /home/me/docs/archive please";
    let kept = |paths: serde_json::Value| async move {
        derive(&with(json!({ "paths": paths })), said)
            .await
            .expect("policy")
            .paths
    };
    assert_eq!(kept(json!(["/home/me/docs/archive"])).await.len(), 1);
    assert_eq!(kept(json!(["/home/me/docs/archive/"])).await.len(), 1);
    for piece in [
        "/",
        "~",
        "",
        "/home",
        "/home/me",
        "docs",
        "/home/me/docs",
        "/etc",
    ] {
        assert_eq!(kept(json!([piece])).await, [], "{piece:?}");
    }
}

#[tokio::test]
async fn a_count_and_a_ceiling_are_bounded_and_unknown_actions_drop() {
    let reply = with(json!({
        "actions": ["org.quire.Banking bank.transfer.send", "org.quire.Mail mail.thread.nuke", "org.quire.Mail mail.thread.read"],
        "apps": [{ "app": "org.quire.Banking", "up_to": "destructive" }],
        "ceiling": "destructive",
        "max_count": 4_294_967_295_u32,
    }));
    let policy = derive(&reply, "summarise this thread")
        .await
        .expect("policy");
    assert_eq!(
        policy.actions.len(),
        1,
        "only the action that exists: {:?}",
        policy.actions
    );
    assert_eq!(
        policy.ceiling,
        Effect::Read,
        "never above what the chosen actions need"
    );
    assert_eq!(policy.max_count.0, 100);
}

#[tokio::test]
async fn replies_outside_the_shape_are_no_policy() {
    let good = with(json!({}));
    let table: Vec<(&str, String)> = vec![
        ("empty", String::new()),
        ("fenced", format!("```json\n{good}\n```")),
        ("trailing words", format!("{good} Done!")),
        ("leading words", format!("Here: {good}")),
        ("null", "null".into()),
        ("an array of drafts", format!("[{good}]")),
        ("a negative count", with(json!({ "max_count": -1 }))),
        (
            "a count that is words",
            with(json!({ "max_count": "many" })),
        ),
        ("a fractional count", with(json!({ "max_count": 1.5 }))),
        (
            "a ceiling that is not an effect",
            with(json!({ "ceiling": "root" })),
        ),
        (
            "a key twice",
            good.replace(
                "\"ceiling\":\"read\"",
                "\"ceiling\":\"destructive\",\"ceiling\":\"read\"",
            ),
        ),
        (
            "recipients as one string",
            with(json!({ "recipients": "eve@evil.test" })),
        ),
        (
            "actions as objects",
            with(json!({ "actions": [{ "a": 1 }] })),
        ),
    ];
    for (name, reply) in table {
        assert_eq!(
            derive(&reply, "summarise this thread").await.map(|_| ()),
            Err(ReviewError::Unparseable),
            "{name}"
        );
    }
}

#[tokio::test]
async fn a_very_large_reply_is_read_in_full_or_refused_never_half_believed() {
    let padding = "x".repeat(2_000_000);
    let big = with(json!({ "kinds": [padding] }));
    // Whatever the length, the answer is a policy with the bounded fields or an error: the
    // kinds that do not parse as kinds are dropped.
    let policy = derive(&big, "summarise this thread").await.expect("policy");
    assert!(policy.kinds.is_empty());
    assert_eq!(policy.ceiling, Effect::Read);
}
