//! What `quire-do` prints: JSON goldens (`tests/golden`, rewritten with `UPDATE_GOLDEN=1`),
//! text on a terminal and JSON when stdout is a pipe, standard input end to end, `describe`
//! through stoker's `Shape::to_json_schema`, search, context and shell completion.

use crate::support::*;
use docket_cli::{Exit, Stdin, Stdout};
use docket_core::{
    ContextSnapshot, EntityRef, Here, Selection, TextTarget, Visible, WindowPrivacy,
};
use porter_core::{AppName, Count};
use prov::{DataClass, EntityId, EntityKey, EntityKind, Label, Labelled, Source, SpaceId};
use serde_json::Value;
use std::path::PathBuf;

fn golden(name: &str, actual: &Value) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.json"));
    let text = format!("{}\n", serde_json::to_string_pretty(actual).expect("json"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &text).expect("write golden");
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e} (run with UPDATE_GOLDEN=1)", path.display()));
    assert_eq!(text, want, "{name} differs from tests/golden/{name}.json");
}

#[tokio::test]
async fn json_goldens() {
    let desk = Desk::answering(vec![once()]);
    golden("apps", &json(&desk.quire(&["apps"]).await));
    golden("list-mail", &json(&desk.quire(&["mail", "--list"]).await));
    golden(
        "describe-message-send",
        &json(&desk.quire(&["describe", "mail", "message.send"]).await),
    );
    golden(
        "outcome-read",
        &json(&desk.quire(&["mail", "thread.read", "t2"]).await),
    );
    golden(
        "outcome-archive",
        &json(&desk.quire(&["mail", "thread.archive", "t2"]).await),
    );
    golden(
        "dry-run-forward",
        &json(
            &desk
                .quire(&[
                    "mail",
                    "message.forward",
                    "t1",
                    "--to",
                    "mail.contact:c1",
                    "--dry-run",
                ])
                .await,
        ),
    );
    golden(
        "error-declined",
        &json(&desk.quire(&["mail", "thread.archive", "t1"]).await),
    );
    golden(
        "error-usage",
        &json(
            &desk
                .quire(&["mail", "thread.read", "t1", "--bogus", "x"])
                .await,
        ),
    );
}

#[tokio::test]
async fn text_on_a_terminal_and_json_on_a_pipe_unless_asked() {
    let desk = Desk::answering(vec![once()]);
    let tty = desk.tty(&["mail", "thread.archive", "t2"]).await;
    assert_eq!(tty.exit, Exit::Done);
    assert!(
        tty.stdout.starts_with("Archived 1 threads\n"),
        "{}",
        tty.stdout
    );
    assert!(
        tty.stdout.contains("undo: quire-do undo "),
        "{}",
        tty.stdout
    );
    assert!(
        serde_json::from_str::<Value>(&tty.stdout).is_err(),
        "text is not JSON"
    );

    let pipe = desk.quire(&["mail", "thread.read", "t1"]).await;
    assert_eq!(
        json(&pipe)["vocab"],
        1,
        "a pipe gets JSON without being asked"
    );

    let asked = desk
        .invoke(
            &["mail", "thread.read", "t1", "--json"],
            Stdin::Closed,
            Stdout::Tty,
        )
        .await;
    assert_eq!(json(&asked)["vocab"], 1, "--json on a terminal gets JSON");

    let failing = desk.tty(&["mail", "thread.read", "nope"]).await;
    assert_eq!(failing.stdout, "");
    assert_eq!(failing.stderr, "quire-do: the app cannot find that thing\n");
}

#[tokio::test]
async fn text_piped_to_a_dash_reaches_the_app() {
    let desk = Desk::answering(vec![once()]);
    let report = desk
        .piped(
            &["mail", "draft.create", "--body", "-"],
            "Dear Accounting,\nthe receipts.\n",
        )
        .await;
    assert_eq!(report.exit, Exit::Done, "{report:?}");
    assert_eq!(desk.router.seams.link.mail.drafts(), 1);
    // The sheet quoted the words as the terminal's own (untrusted, from the CLI).
    let sheet = &desk.router.seams.confirmer.requests()[0];
    let line = sheet.lines.first().expect("the body line");
    assert!(
        matches!(&line.value, docket_core::Shown::Quoted { from: Source::Cli, text } if text == "Dear Accounting,\nthe receipts."),
        "{line:?}"
    );
}

#[tokio::test]
async fn nothing_piped_to_a_dash_is_a_usage_error() {
    let desk = Desk::new();
    let report = desk.quire(&["mail", "draft.create", "--body", "-"]).await;
    assert_eq!(report.exit, Exit::Usage);
    assert!(report.stdout.contains("nothing was piped in"), "{report:?}");
    assert_eq!(desk.sheets(), 0, "and nothing was sent to the router");
}

#[tokio::test]
async fn describe_prints_the_schema_stoker_renders() {
    let desk = Desk::new();
    let described = json(&desk.quire(&["describe", "mail", "message.send"]).await);
    let schema = &described["schema"];
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["required"], serde_json::json!(["to", "body"]));
    assert_eq!(schema["properties"]["body"]["type"], "string");
    assert_eq!(schema["properties"]["body"]["maxLength"], 20000);
    assert_eq!(described["effect"], "outbound");
    let human = desk.tty(&["describe", "mail", "message.send"]).await;
    assert!(human.stdout.contains("\"properties\""), "{}", human.stdout);
}

#[tokio::test]
async fn search_prints_the_apps_things_and_context_prints_what_it_shows() {
    let desk = Desk::new();
    desk.index_mail().await;
    let found = desk.quire(&["mail", "search", "receipts"]).await;
    assert_eq!(found.exit, Exit::Done, "{found:?}");
    let doc = json(&found);
    assert_eq!(doc["entities"][0]["key"], "t2", "{doc}");
    let text = desk.tty(&["mail", "search", "receipts"]).await;
    assert!(
        text.stdout.starts_with("mail.thread:t2  Lisbon receipts"),
        "{}",
        text.stdout
    );
    // Another app's hits are not this app's.
    let files = json(&desk.quire(&["files", "search", "receipts"]).await);
    assert_eq!(files["hits"], serde_json::json!([]));

    let mail = AppName::parse("org.quire.Mail").expect("app");
    let thread = EntityId {
        app: mail.clone(),
        kind: EntityKind::parse("mail.thread").expect("kind"),
        key: EntityKey::parse("t1").expect("key"),
    };
    let shown = Label::untrusted(
        Source::Mail,
        DataClass::Mail,
        SpaceId::parse("work").expect("space"),
    );
    desk.router.seams.link.show_window(ContextSnapshot {
        app: mail,
        window: Labelled {
            value: "Inbox".into(),
            label: Label::trusted_user(),
        },
        here: Here::Nowhere,
        selection: Selection::Nothing,
        visible: Visible {
            kind: Some(thread.kind.clone()),
            items: vec![EntityRef {
                id: thread,
                title: Labelled {
                    value: "Invoice".into(),
                    label: shown.clone(),
                },
                subtitle: Labelled {
                    value: "eve@evil.test".into(),
                    label: shown,
                },
            }],
            total: Count(1),
        },
        text_target: TextTarget::None,
        privacy: WindowPrivacy::Normal,
    });
    let view = desk.quire(&["mail", "context"]).await;
    assert_eq!(view.exit, Exit::Done, "{view:?}");
    let doc = json(&view);
    // Somebody else's words come back as handles, usable by a later command as `#n`.
    let handle = doc["handles"][0]
        .as_u64()
        .expect("a handle for the untrusted title");
    assert_eq!(doc["entities"][0]["key"], "t1");
    let text = desk.tty(&["mail", "context"]).await;
    assert!(text.stdout.contains("app: mail"), "{}", text.stdout);
    assert!(
        text.stdout.contains("visible: mail.thread:t1  #"),
        "{}",
        text.stdout
    );
    // The held text feeds a later call, with the label it was held with.
    let draft = desk
        .quire(&["mail", "draft.create", "--body", &format!("#{handle}")])
        .await;
    assert_eq!(
        draft.exit,
        Exit::Declined,
        "it asks, as every write from a terminal does: {draft:?}"
    );
    let sheet = desk.router.seams.confirmer.requests();
    assert!(
        matches!(&sheet[0].lines[0].value, docket_core::Shown::Quoted { text, .. } if text == "Invoice" || text == "eve@evil.test"),
        "{:?}",
        sheet[0].lines
    );
}

#[tokio::test]
async fn completion_follows_the_installed_manifests() {
    let desk = Desk::new();
    async fn complete(desk: &Desk, words: &[&str]) -> Vec<String> {
        let mut all = vec!["__complete"];
        all.extend_from_slice(words);
        let report = desk.quire(&all).await;
        assert_eq!(report.exit, Exit::Done, "{report:?}");
        report.stdout.lines().map(str::to_owned).collect()
    }
    let top = complete(&desk, &[""]).await;
    for want in ["apps", "describe", "undo", "mail", "files", "memory"] {
        assert!(top.iter().any(|w| w == want), "{want} in {top:?}");
    }
    assert_eq!(complete(&desk, &["me"]).await, ["memory"]);
    let actions = complete(&desk, &["mail", "thread."]).await;
    assert!(
        actions.contains(&"thread.archive".to_owned()),
        "{actions:?}"
    );
    assert!(
        !complete(&desk, &["memory", ""])
            .await
            .contains(&"forget".to_owned()),
        "a hidden action is not offered"
    );
    let flags = complete(&desk, &["mail", "message.send", "t1", "--"]).await;
    assert!(
        flags.contains(&"--to".to_owned()) && flags.contains(&"--dry-run".to_owned()),
        "{flags:?}"
    );
    assert!(!flags.contains(&"--yes".to_owned()));
    assert_eq!(
        complete(&desk, &["describe", "mail", "message.f"]).await,
        ["message.forward"]
    );
}
