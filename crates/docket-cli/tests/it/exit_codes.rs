//! The exit codes, one row per way a run can end (cli.md section 4), against the fake router.
//! Run with `--nocapture` to read the table.

use crate::support::*;
use docket_cli::{Exit, Invocation, Stdin, Stdout, run};
use docket_client::{Intents, Transport, TransportError};
use docket_core::{ConfirmAnswer, ConfirmEnd, IntentsReply, IntentsRequest};
use docket_fake::Answering;
use porter_core::AppName;

/// The intentd that is not there.
struct Down;

impl Transport for Down {
    async fn call(&self, _: IntentsRequest) -> Result<IntentsReply, TransportError> {
        Err(TransportError::Closed)
    }
}

fn ended(end: ConfirmEnd) -> ConfirmAnswer {
    ConfirmAnswer::Ended(end)
}

#[tokio::test]
async fn every_way_a_run_ends_has_its_code() {
    struct Row {
        what: &'static str,
        words: &'static [&'static str],
        answers: Vec<ConfirmAnswer>,
        want: Exit,
    }
    let rows = vec![
        Row {
            what: "a read",
            words: &["mail", "thread.read", "mail.thread:t2"],
            answers: vec![],
            want: Exit::Done,
        },
        Row {
            what: "an undoable write the person allows once",
            words: &["mail", "thread.archive", "t2"],
            answers: vec![once()],
            want: Exit::Done,
        },
        Row {
            what: "the list of apps",
            words: &["apps"],
            answers: vec![],
            want: Exit::Done,
        },
        Row {
            what: "an unknown app",
            words: &["nope", "thread.read", "t1"],
            answers: vec![],
            want: Exit::Usage,
        },
        Row {
            what: "an unknown action",
            words: &["mail", "thread.explode", "t1"],
            answers: vec![],
            want: Exit::Usage,
        },
        Row {
            what: "an unknown parameter",
            words: &["mail", "thread.read", "t1", "--bogus", "x"],
            answers: vec![],
            want: Exit::Usage,
        },
        Row {
            what: "a required parameter missing",
            words: &["mail", "draft.create"],
            answers: vec![],
            want: Exit::Usage,
        },
        Row {
            what: "a thing of the wrong kind",
            words: &[
                "mail",
                "message.send",
                "--to",
                "mail.thread:t1",
                "--body",
                "hi",
            ],
            answers: vec![],
            want: Exit::Usage,
        },
        Row {
            what: "no thing named for a Many target",
            words: &["mail", "thread.archive"],
            answers: vec![],
            want: Exit::Usage,
        },
        Row {
            what: "nothing at all",
            words: &[],
            answers: vec![],
            want: Exit::Usage,
        },
        Row {
            what: "a hidden action",
            words: &["memory", "forget", "memory.fact:f1"],
            answers: vec![],
            want: Exit::Refused,
        },
        Row {
            what: "an undoable write the person declines",
            words: &["mail", "thread.archive", "t2"],
            answers: vec![ended(ConfirmEnd::Refused)],
            want: Exit::Declined,
        },
        Row {
            what: "an undoable write nobody answers",
            words: &["mail", "thread.archive", "t2"],
            answers: vec![],
            want: Exit::Declined,
        },
        Row {
            what: "a confirmation that expires",
            words: &["mail", "thread.archive", "t2"],
            answers: vec![ended(ConfirmEnd::Expired)],
            want: Exit::Declined,
        },
        Row {
            what: "an outbound send the person declines",
            words: &[
                "mail",
                "message.send",
                "--to",
                "mail.contact:c1",
                "--body",
                "hi",
            ],
            answers: vec![ended(ConfirmEnd::Dismissed)],
            want: Exit::Declined,
        },
        Row {
            what: "a thing the app cannot find",
            words: &["mail", "thread.read", "missing"],
            answers: vec![],
            want: Exit::AppFailed,
        },
        Row {
            what: "an app that cannot show a window",
            words: &["mail", "context"],
            answers: vec![],
            want: Exit::Unavailable,
        },
    ];
    println!("row                                            exit  word");
    for row in rows {
        let desk = Desk::answering(row.answers);
        let report = desk.quire(row.words).await;
        println!(
            "{:<46} {:>4}  {}",
            row.what,
            report.exit.code(),
            report.exit.word()
        );
        assert_eq!(report.exit, row.want, "{}: {report:?}", row.what);
        if row.want != Exit::Done {
            assert!(
                report.stdout.contains("\"error\""),
                "{}: {}",
                row.what,
                report.stdout
            );
        }
    }
}

#[tokio::test]
async fn a_halted_desktop_is_exit_7_and_so_is_a_paused_session() {
    let desk = Desk::new();
    desk.halt().await;
    let report = desk.quire(&["mail", "thread.read", "t2"]).await;
    println!(
        "{:<46} {:>4}  {}",
        "everything halted",
        report.exit.code(),
        report.exit.word()
    );
    assert_eq!(report.exit, Exit::Halted, "{report:?}");

    // Three refusals in a row pause the session until the person speaks.
    let desk = Desk::new();
    for key in ["t1", "t2", "t3", "t4"] {
        desk.router
            .seams
            .link
            .mail
            .add_thread(docket_fake::MailThread {
                key: key.into(),
                subject: "s".into(),
                from: "a@example.test".into(),
                body: "b".into(),
            });
    }
    let mut last = Exit::Done;
    for key in ["t1", "t2", "t3", "t4"] {
        last = desk.quire(&["mail", "thread.archive", key]).await.exit;
    }
    println!(
        "{:<46} {:>4}  {}",
        "paused by the breaker",
        last.code(),
        last.word()
    );
    assert_eq!(last, Exit::Halted);
}

#[tokio::test]
async fn an_app_that_does_not_answer_is_the_app_failing_the_call() {
    let desk = Desk::new();
    let mail = AppName::parse("org.quire.Mail").expect("app");
    desk.router.seams.link.answer_from(&mail, Answering::Silent);
    let report = desk.quire(&["mail", "thread.read", "t2"]).await;
    println!(
        "{:<46} {:>4}  {}",
        "an app that times out",
        report.exit.code(),
        report.exit.word()
    );
    assert_eq!(report.exit, Exit::AppFailed, "{report:?}");
}

#[tokio::test]
async fn an_app_that_is_not_installed_is_exit_6_not_5() {
    let desk = Desk::answering(vec![once()]);
    desk.router.seams.link.answer_from(
        &AppName::parse("org.quire.Mail").expect("app"),
        docket_fake::Answering::Absent,
    );
    let report = desk.quire(&["mail", "thread.archive", "t1"]).await;
    assert_eq!(report.exit, Exit::Unavailable, "{report:?}");
}

#[tokio::test]
async fn intentd_being_down_is_exit_6_for_every_command() {
    let intents = Intents::over(Down);
    for words in [
        vec!["apps"],
        vec!["mail", "--list"],
        vec!["mail", "thread.read", "t1"],
        vec!["mail", "search", "x"],
        vec!["undo", "--last"],
        vec!["undo", "3"],
    ] {
        let report = run(
            &intents,
            Invocation {
                words: words.iter().map(|w| (*w).to_owned()).collect(),
                stdin: Stdin::Closed,
                stdout: Stdout::Tty,
            },
        )
        .await;
        println!(
            "{:<46} {:>4}  {}",
            format!("{words:?} with intentd down"),
            report.exit.code(),
            report.exit.word()
        );
        assert_eq!(report.exit, Exit::Unavailable, "{words:?}: {report:?}");
        assert!(report.stderr.starts_with("quire-do: "), "{report:?}");
    }
}

#[test]
fn the_codes_are_the_table_in_cli_md() {
    assert_eq!(
        Exit::ALL.map(Exit::code),
        [0, 2, 3, 4, 5, 6, 7, 8],
        "stable: scripts depend on them"
    );
    let help = docket_cli::Failure::usage("x").json();
    assert_eq!(help["error"]["exit"], 2);
}
