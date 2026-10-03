//! `quire-do` goes through the same gate as everything else: a terminal cannot tell the person
//! from an agent typing in it, so a read runs, every other act asks, a hidden action is refused,
//! and the only way to skip the question is the person's own answer on the sheet.

mod support;

use docket_cli::Exit;
use docket_core::{ActionRef, AskReason, ConfirmOffer};
use prov::Effect;
use support::*;

fn archive_ref() -> ActionRef {
    ActionRef {
        app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
        name: prov::ActionName::parse("mail.thread.archive").expect("action"),
    }
}

#[tokio::test]
async fn a_cli_outbound_call_asks_and_the_sheet_says_it_came_from_the_terminal() {
    let desk = Desk::new();
    let report = desk
        .quire(&[
            "mail",
            "message.send",
            "--to",
            "mail.contact:c1",
            "--body",
            "Receipts attached.",
        ])
        .await;
    assert_eq!(report.exit, Exit::Declined, "{report:?}");
    let sheets = desk.router.seams.confirmer.requests();
    assert_eq!(sheets.len(), 1, "the person is asked");
    assert_eq!(sheets[0].actor, prov::Actor::Cli);
    assert_eq!(sheets[0].effect, Effect::Outbound);
    assert!(
        sheets[0].why.contains(&AskReason::FromTerminal),
        "{:?}",
        sheets[0].why
    );
    assert_eq!(
        sheets[0].offer,
        ConfirmOffer::OnceOnly,
        "a recipient typed in a terminal is untrusted: no standing grant can cover it"
    );
    assert!(
        desk.router.seams.link.mail.sent().is_empty(),
        "and nothing was sent"
    );
    assert!(
        desk.router.seams.reviewer.calls().is_empty(),
        "no reviewer approves for a terminal"
    );
}

#[tokio::test]
async fn an_outbound_call_the_person_allows_runs_once() {
    let desk = Desk::answering(vec![once()]);
    let args = [
        "mail",
        "message.send",
        "--to",
        "mail.contact:c1",
        "--body",
        "hi",
    ];
    assert_eq!(desk.quire(&args).await.exit, Exit::Done);
    assert_eq!(desk.router.seams.link.mail.sent().len(), 1);
    assert_eq!(
        desk.quire(&args).await.exit,
        Exit::Declined,
        "the next one asks again"
    );
    assert_eq!(desk.sheets(), 2);
}

#[tokio::test]
async fn a_standing_grant_skips_the_ask_and_nothing_else_does() {
    let desk = Desk::answering(vec![from_terminal()]);
    let first = desk.quire(&["mail", "thread.archive", "t2"]).await;
    assert_eq!(first.exit, Exit::Done, "{first:?}");
    assert_eq!(desk.sheets(), 1);
    assert_eq!(
        desk.router.seams.confirmer.requests()[0].offer,
        ConfirmOffer::OnceOrFromTerminal,
        "the sheet offered \"allow from the terminal until logout\""
    );
    assert_eq!(desk.router.terminal_grants(), [archive_ref()]);
    let again = desk.quire(&["mail", "thread.archive", "t1"]).await;
    assert_eq!(again.exit, Exit::Done, "{again:?}");
    assert_eq!(desk.sheets(), 1, "the grant skipped the ask");
    // Another action asks.
    let other = desk.quire(&["mail", "thread.delete", "t1"]).await;
    assert_eq!(other.exit, Exit::Declined);
    assert_eq!(desk.sheets(), 2);
    // Logout ends it.
    desk.router.end_terminal_sessions();
    let after = desk.quire(&["mail", "thread.archive", "t1"]).await;
    assert_eq!(after.exit, Exit::Declined, "{after:?}");
    assert_eq!(desk.sheets(), 3, "after logout it asks again");
}

#[tokio::test]
async fn a_hidden_action_is_refused_and_unlisted() {
    let desk = Desk::new();
    let report = desk.quire(&["memory", "forget", "memory.fact:f1"]).await;
    assert_eq!(report.exit, Exit::Refused, "{report:?}");
    assert_eq!(desk.sheets(), 0, "nobody is asked about a hidden action");
    let listed = json(&desk.quire(&["memory", "--list"]).await);
    let names: Vec<&str> = listed["actions"]
        .as_array()
        .expect("actions")
        .iter()
        .filter_map(|a| a["action"].as_str())
        .collect();
    assert!(!names.contains(&"forget"), "{names:?}");
    assert!(names.contains(&"recall"), "{names:?}");
    let described = desk.quire(&["describe", "memory", "forget"]).await;
    assert_eq!(
        described.exit,
        Exit::Usage,
        "a hidden action is not described either"
    );
}

#[tokio::test]
async fn a_dry_run_prints_the_preview_and_asks_nobody() {
    let desk = Desk::new();
    let report = desk
        .quire(&[
            "mail",
            "message.forward",
            "t2",
            "--to",
            "mail.contact:c1",
            "--dry-run",
        ])
        .await;
    assert_eq!(report.exit, Exit::Done, "{report:?}");
    let doc = json(&report);
    assert_eq!(doc["preview"]["kind"], "message");
    assert_eq!(desk.sheets(), 0);
    assert!(desk.router.seams.link.mail.sent().is_empty());
    // A dry run of a hidden action is refused, not previewed.
    let hidden = desk
        .quire(&["memory", "forget", "memory.fact:f1", "--dry-run"])
        .await;
    assert_eq!(hidden.exit, Exit::Refused, "{hidden:?}");
}

#[tokio::test]
async fn there_is_no_flag_that_skips_the_question() {
    let desk = Desk::new();
    for flag in ["--yes", "-y", "--force", "--no-confirm", "--assume-yes"] {
        let report = desk
            .quire(&["mail", "thread.archive", "t2", flag, "true"])
            .await;
        assert_eq!(report.exit, Exit::Usage, "{flag}: {report:?}");
    }
    assert!(!desk.router.seams.link.mail.is_archived("t2"));
    assert_eq!(desk.sheets(), 0);
}

#[tokio::test]
async fn a_terminal_undoes_only_what_a_terminal_did() {
    let desk = Desk::answering(vec![once()]);
    let done = json(&desk.quire(&["mail", "thread.archive", "t2"]).await);
    let id = done["undo"]["id"]
        .as_u64()
        .expect("an undo id in the output");
    assert!(desk.router.seams.link.mail.is_archived("t2"));
    let undone = desk.quire(&["undo", &id.to_string()]).await;
    assert_eq!(undone.exit, Exit::Done, "{undone:?}");
    assert!(!desk.router.seams.link.mail.is_archived("t2"));
    // Nothing left to undo from here.
    let none = desk.quire(&["undo", "--last"]).await;
    assert_eq!(none.exit, Exit::Usage, "{none:?}");
}

#[tokio::test]
async fn undo_last_finds_the_newest_row_of_this_terminal() {
    let desk = Desk::answering(vec![once(), once()]);
    desk.quire(&["mail", "thread.archive", "t1"]).await;
    desk.quire(&["mail", "thread.archive", "t2"]).await;
    let undone = desk.quire(&["undo", "--last"]).await;
    assert_eq!(undone.exit, Exit::Done, "{undone:?}");
    assert!(
        !desk.router.seams.link.mail.is_archived("t2"),
        "the newest came back"
    );
    assert!(desk.router.seams.link.mail.is_archived("t1"));
}
