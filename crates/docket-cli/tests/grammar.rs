//! The command line's own grammar: which words are the command, the flags and the parameters,
//! before any manifest is read.

use docket_cli::{Command, JsonFlag, RunMode, UndoWhich, parse};

fn command(words: &[&str]) -> Result<Command, String> {
    let words: Vec<String> = words.iter().map(|w| (*w).to_owned()).collect();
    parse(&words).map(|p| p.command).map_err(|f| f.what)
}

#[test]
fn the_command_line_grammar() {
    assert_eq!(command(&["apps"]), Ok(Command::Apps));
    assert_eq!(
        command(&["mail", "--list"]),
        Ok(Command::List { app: "mail".into() })
    );
    assert_eq!(
        command(&["describe", "mail", "thread.archive"]),
        Ok(Command::Describe {
            app: "mail".into(),
            action: "thread.archive".into()
        })
    );
    assert_eq!(
        command(&["mail", "search", "lisbon", "receipts"]),
        Ok(Command::Search {
            app: "mail".into(),
            text: "lisbon receipts".into()
        })
    );
    assert_eq!(
        command(&["mail", "context"]),
        Ok(Command::Context { app: "mail".into() })
    );
    assert_eq!(
        command(&["undo", "7"]),
        Ok(Command::Undo(UndoWhich::Entry(7)))
    );
    assert_eq!(
        command(&["undo", "--last"]),
        Ok(Command::Undo(UndoWhich::Last))
    );
    assert_eq!(command(&["--help"]), Ok(Command::Help));
    assert_eq!(
        command(&["mail", "thread.read", "--help"]),
        Ok(Command::Help)
    );
    match command(&[
        "mail",
        "message.send",
        "--to=mail.contact:c1",
        "--body",
        "hi",
        "--dry-run",
        "--session",
        "s-9",
        "t1",
    ]) {
        Ok(Command::Call(call)) => {
            assert_eq!(call.app, "mail");
            assert_eq!(call.action, "message.send");
            assert_eq!(call.mode, RunMode::DryRun);
            assert_eq!(call.session.expect("session").as_str(), "s-9");
            assert_eq!(
                call.params,
                [
                    ("to".to_owned(), "mail.contact:c1".to_owned()),
                    ("body".to_owned(), "hi".to_owned())
                ]
            );
            assert_eq!(call.targets, ["t1"]);
        }
        other => panic!("{other:?}"),
    }
    // Flags may come first, last or between; dashes and underscores are the same flag.
    match command(&["--in-reply-to", "x", "mail", "message.send"]) {
        Ok(Command::Call(call)) => {
            assert_eq!(call.params, [("in_reply_to".to_owned(), "x".to_owned())])
        }
        other => panic!("{other:?}"),
    }
    assert!(
        command(&["undo", "soon"])
            .expect_err("id")
            .contains("undo id")
    );
    assert!(command(&["mail"]).is_err());
    assert!(
        command(&["mail", "thread.read", "-y"])
            .expect_err("short")
            .contains("unknown option")
    );
    assert!(
        command(&["mail", "thread.read", "--body"])
            .expect_err("value")
            .contains("needs a value")
    );
    let json = parse(&["apps".into(), "--json".into()]).expect("parsed");
    assert_eq!(json.json, JsonFlag::Asked);
}
