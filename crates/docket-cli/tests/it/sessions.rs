//! `quire-do sessions`: list, load and fork through `Session.Stored`, over the fake router as the
//! cli role. The router lists and pages only the sessions a terminal may bring back (the ones
//! its own app opened), so another app's session is not listed, not loadable and not forkable.

use crate::support::{Desk, caller, json};
use docket_cli::Exit;
use docket_core::{
    CallerRole, ContextKeep, IntentsReply, IntentsRequest, Keep, Origin, SessionOpen, TurnIn,
    TurnVia,
};
use prov::{AgentRef, SessionId, SpaceId};

fn keep_nothing() -> ContextKeep {
    ContextKeep {
        query: Keep::Dropped,
        results: Keep::Dropped,
        selection: Keep::Dropped,
        window: Keep::Dropped,
    }
}

/// A session opened by `app` (as the launcher: only the opener's name matters to the rule) with
/// one turn of the person's words.
async fn session_of(desk: &Desk, app: &str, words: &str) -> SessionId {
    let who = caller(app, CallerRole::Launcher);
    let opened = desk
        .router
        .handle(
            &who,
            IntentsRequest::SessionOpen(SessionOpen {
                space: SpaceId::parse("work").expect("space"),
                agent: AgentRef::Companion,
                parent: None,
                cwd: None,
                started_from: None,
                external: None,
            }),
        )
        .await;
    let IntentsReply::SessionOpened(opened) = opened else {
        panic!("{opened:?}")
    };
    let turn = desk
        .router
        .handle(
            &who,
            IntentsRequest::SessionTurn {
                session: opened.session.clone(),
                turn: TurnIn {
                    text: words.into(),
                    origin: Origin::Launcher,
                    keep: keep_nothing(),
                    via: TurnVia::Typed,
                },
            },
        )
        .await;
    assert!(matches!(turn, IntentsReply::TurnRecorded(_)), "{turn:?}");
    opened.session
}

fn ids(report: &docket_cli::Report) -> Vec<String> {
    json(report)["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .map(|s| s["session"].as_str().expect("id").to_owned())
        .collect()
}

#[tokio::test]
async fn nothing_to_list_is_said_plainly() {
    let desk = Desk::new();
    let tty = desk.tty(&["sessions"]).await;
    assert_eq!(tty.exit, Exit::Done);
    assert_eq!(tty.stdout, "no sessions\n");
    assert_eq!(ids(&desk.quire(&["sessions"]).await), Vec::<String>::new());
}

#[tokio::test]
async fn the_list_holds_the_terminals_own_sessions_and_not_another_apps() {
    let desk = Desk::new();
    let mine = session_of(&desk, "org.quire.Do", "tidy the downloads").await;
    let theirs = session_of(&desk, "org.quire.Shell", "something else").await;

    let tty = desk.tty(&["sessions"]).await;
    assert_eq!(tty.exit, Exit::Done, "{tty:?}");
    assert_eq!(
        tty.stdout,
        format!("{mine}  work  companion  1 turn  open\n")
    );
    let listed = ids(&desk.quire(&["sessions"]).await);
    assert_eq!(listed, [mine.to_string()]);

    // The other app's session is answered as one the log does not hold.
    for words in [
        vec!["sessions", "load", theirs.as_str()],
        vec!["sessions", "fork", theirs.as_str()],
    ] {
        let refused = desk.tty(&words).await;
        assert_ne!(refused.exit, Exit::Done, "{words:?}: {refused:?}");
        assert!(refused.stdout.is_empty());
    }
}

#[tokio::test]
async fn load_prints_the_turns_and_json_is_the_export() {
    let desk = Desk::new();
    let mine = session_of(&desk, "org.quire.Do", "tidy the downloads").await;

    let tty = desk.tty(&["sessions", "load", mine.as_str()]).await;
    assert_eq!(tty.exit, Exit::Done, "{tty:?}");
    assert_eq!(tty.stdout, "opened in work\nyou: tidy the downloads\n");

    let doc = json(&desk.quire(&["sessions", "load", mine.as_str()]).await);
    assert_eq!(doc["session"], mine.as_str());
    assert!(doc["entries"].as_array().is_some_and(|e| e.len() >= 2));
}

#[tokio::test]
async fn a_fork_keeps_the_log_up_to_a_row_and_is_the_terminals_too() {
    let desk = Desk::new();
    let mine = session_of(&desk, "org.quire.Do", "tidy the downloads").await;

    let forked = desk.quire(&["sessions", "fork", mine.as_str()]).await;
    assert_eq!(forked.exit, Exit::Done, "{forked:?}");
    let child = json(&forked)["session"].as_str().expect("child").to_owned();
    assert_eq!(child, format!("{mine}-f1"));

    // The child is listed and loads with the parent's words.
    assert_eq!(
        ids(&desk.quire(&["sessions"]).await),
        [mine.to_string(), child.clone()]
    );
    let tty = desk.tty(&["sessions", "load", &child]).await;
    assert!(tty.stdout.contains("you: tidy the downloads"), "{tty:?}");

    // A second fork gets the next name; a row the log does not have is refused.
    let again = desk
        .tty(&["sessions", "fork", mine.as_str(), "--at", "0"])
        .await;
    assert_eq!(again.exit, Exit::Done, "{again:?}");
    assert_eq!(again.stdout, format!("{mine}-f2\n"));
    let past = desk
        .tty(&["sessions", "fork", mine.as_str(), "--at", "99"])
        .await;
    assert_ne!(past.exit, Exit::Done, "{past:?}");
}

#[tokio::test]
async fn the_grammar_refuses_what_it_cannot_read() {
    let desk = Desk::new();
    for words in [
        vec!["sessions", "load"],
        vec!["sessions", "load", "not a session"],
        vec!["sessions", "fork", "s-1", "--at", "x"],
        vec!["sessions", "fork", "s-1", "--to", "3"],
        vec!["sessions", "delete", "s-1"],
    ] {
        let report = desk.tty(&words).await;
        assert_eq!(report.exit, Exit::Usage, "{words:?}: {report:?}");
    }
}
