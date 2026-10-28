//! Helpers that run an agent app, and the terminal: a conversation moves to the app's own program
//! in a terminal and comes back, one that cannot move says why, and a conversation a terminal
//! started is recognised and brought in.

use crate::support::apps::{
    Apps, app_draft, done, done_resumable, give, hub, make, program, state_of, status_of,
    terminal_hand, until,
};
use crate::support::world::*;
use companion_model::helper::*;
use companion_model::{AppSessionId, SeenCommand, WatchedAgent};
use companion_wire::{BroughtBack, Handover, HelperReply, RunsIn};
use docket_client::{InProcess, Intents};
use docket_core::Rewind;
use docket_permissions_core::ProgramName;
use docket_tasks::HelperDesk;
use docket_vocab::{CallerRole, Workspace};
use prov::SessionId;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn a_conversation_opens_in_the_terminal_pauses_its_helper_and_comes_back() {
    let w = world(vec![]);
    let apps = Arc::new(Apps::default());
    apps.will_run(
        "Coder",
        vec![done_resumable("First."), done_resumable("Second.")],
    );
    let hub = hub(&w, &apps);
    let coder = make(&hub, "Coder").await;
    // Nothing to take along before a job has run.
    let early = hub.open_in_terminal(coder.clone()).await;
    assert!(
        matches!(&early, HelperReply::Refused(why) if why.contains("Give it a job first")),
        "{early:?}"
    );
    let first = give(&hub, &coder, "compare the offers").await;
    until("the job is done", || {
        matches!(status_of(&hub, first), JobStatus::Done(_))
    })
    .await;
    let card = hub.card(&coder).expect("card");
    assert_eq!(
        (card.runs_in, card.handover),
        (RunsIn::Desktop, Handover::ToTerminal)
    );

    // Open in Terminal: the window to open carries the resume line and names the session.
    let hand = terminal_hand();
    apps.to_terminal
        .lock()
        .expect("to terminal")
        .push_back(ToTerminalOutcome::Ready(Box::new(hand.clone())));
    let reply = hub.open_in_terminal(coder.clone()).await;
    assert_eq!(
        reply,
        HelperReply::OpenTerminal(TerminalRequest {
            helper: coder.clone(),
            program: program(),
            login: TerminalLogin {
                run: Some(Box::new(hand.run.clone())),
                ..TerminalLogin::default()
            },
            run: Some(Box::new(hand.run)),
            handoff: Some(hand.session),
        })
    );
    let away = hub.card(&coder).expect("card");
    assert_eq!(
        (away.runs_in, away.handover),
        (RunsIn::Terminal, Handover::BringBack)
    );
    assert_eq!(away.line.doing, "In its own terminal window");
    assert_eq!(hub.view(0).helpers[0].doing, "In its own terminal window");

    // A job given meanwhile waits: no job reaches the app while the person works in it.
    let second = give(&hub, &coder, "and the best one?").await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(status_of(&hub, second), JobStatus::Open);
    let again = hub.open_in_terminal(coder.clone()).await;
    assert!(
        matches!(&again, HelperReply::Refused(why) if why.contains("already in the terminal")),
        "{again:?}"
    );

    // Bring Back: the helper goes on and takes the job that waited.
    apps.back.lock().expect("back").push_back(BackOutcome::Back);
    assert_eq!(hub.bring_back(coder.clone()).await, HelperReply::Done);
    until("the second job is done", || {
        matches!(status_of(&hub, second), JobStatus::Done(_))
    })
    .await;
    assert_eq!(hub.card(&coder).expect("card").runs_in, RunsIn::Desktop);
}

#[tokio::test]
async fn a_conversation_that_cannot_move_stays_put_and_says_why_in_plain_words() {
    let w = world(vec![]);
    let apps = Arc::new(Apps::default());
    apps.will_run("Coder", vec![done_resumable("First."), done("No id.")]);
    let hub = hub(&w, &apps);
    let coder = make(&hub, "Coder").await;
    let first = give(&hub, &coder, "compare the offers").await;
    until("the job is done", || {
        matches!(status_of(&hub, first), JobStatus::Done(_))
    })
    .await;

    // The app says it cannot pick the conversation up: the helper keeps working here.
    apps.to_terminal
        .lock()
        .expect("to terminal")
        .push_back(ToTerminalOutcome::Refused(HandFault::NotResumable));
    let refused = hub.open_in_terminal(coder.clone()).await;
    assert_eq!(
        refused,
        HelperReply::Refused("This app can't pick its conversation back up here.".to_owned())
    );
    let card = hub.card(&coder).expect("card");
    assert_eq!(
        (card.runs_in, card.handover),
        (RunsIn::Desktop, Handover::ToTerminal)
    );
    assert_eq!(state_of(&hub, &coder), HelperState::Idle);

    // Bringing back what is not in the terminal, and one the app would not pick up, are refused.
    let not_there = hub.bring_back(coder.clone()).await;
    assert!(
        matches!(&not_there, HelperReply::Refused(_)),
        "{not_there:?}"
    );

    // A job whose app gave no conversation id of its own leaves nothing to take along.
    let second = give(&hub, &coder, "once more").await;
    until("the job is done", || {
        matches!(status_of(&hub, second), JobStatus::Done(_))
    })
    .await;
    assert_eq!(
        hub.card(&coder).expect("card").handover,
        Handover::Unavailable
    );
    let none = hub.open_in_terminal(coder.clone()).await;
    assert!(matches!(&none, HelperReply::Refused(_)), "{none:?}");

    // A helper that runs no app has no program to open.
    let draft = CardDraft {
        kind: HelperKind::Native,
        ..app_draft("Research")
    };
    let effects = hub.apply(BoardEvent::Create(Box::new(draft))).await;
    let research = effects
        .iter()
        .find_map(|e| match e {
            BoardEffect::Created(id) => Some(id.clone()),
            _ => None,
        })
        .expect("a card was made");
    let plain = hub.open_in_terminal(research).await;
    assert_eq!(
        plain,
        HelperReply::Refused("This helper does not run an app.".to_owned())
    );
}

#[tokio::test]
async fn a_command_line_a_terminal_saw_is_recognised_by_the_host_of_the_apps() {
    let w = world(vec![]);
    let apps = Arc::new(Apps::default());
    let seen = |line: &str| {
        SeenCommand::parse(line.split(' ').map(str::to_owned).collect()).expect("seen")
    };
    // With no host of apps there is nothing to recognise.
    assert_eq!(
        hub_of(&w)
            .recognise(seen("claude --resume 0a1b-2c3d"))
            .await,
        None
    );
    let hub = hub(&w, &apps);
    let want = WatchedAgent {
        program: ProgramName::parse("claude-code").expect("program"),
        session: AppSessionId::parse("0a1b-2c3d").expect("id"),
    };
    assert_eq!(
        hub.recognise(seen("claude --resume 0a1b-2c3d")).await,
        Some(want)
    );
    assert_eq!(hub.recognise(seen("vim notes.txt")).await, None);
}

#[tokio::test]
async fn a_conversation_a_terminal_started_is_listed_and_brought_into_docket() {
    let w = world(vec![]);
    let apps = Arc::new(Apps::default());
    let hub = hub(&w, &apps);
    // A terminal that names its agent app and the app's own id for the conversation.
    let temor = Intents::over(InProcess::new(
        w.router.clone(),
        caller("org.quire.Temor", CallerRole::Cli),
    ));
    let named = temor
        .checkpoint_watch_agent(
            Workspace::parse("/work/project").expect("workspace"),
            "claude in pane 2".to_owned(),
            Rewind::Agent,
            WatchedAgent {
                program: ProgramName::parse("claude-code").expect("program"),
                session: AppSessionId::parse("0a1b-2c3d").expect("id"),
            },
        )
        .await
        .expect("watching")
        .session;
    // One that does not name its agent is not offered.
    temor
        .checkpoint_watch(
            Workspace::parse("/work/other").expect("workspace"),
            "something".to_owned(),
            Rewind::Docket,
        )
        .await
        .expect("watching");

    let listed = hub.terminal_sessions().await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].session, named);
    assert_eq!(listed[0].app_name.as_deref(), Some("Claude Code"));
    assert_eq!(listed[0].folder.as_deref(), Some("/work/project"));
    assert_eq!(listed[0].handover, Handover::BringBack);

    apps.brought
        .lock()
        .expect("brought")
        .push_back(BroughtOutcome::Brought(named.clone()));
    assert_eq!(
        hub.bring_back_terminal(named.clone()).await,
        BroughtBack::Session(named.clone())
    );
    assert_eq!(
        apps.brought_with.lock().expect("brought with").clone(),
        [(
            named,
            "0a1b-2c3d".to_owned(),
            Workspace::parse("/work/project").expect("workspace")
        )]
    );
    // One that is not listed is not there.
    let missing = hub
        .bring_back_terminal(SessionId::parse("s-404").expect("session"))
        .await;
    assert!(matches!(missing, BroughtBack::Refused(_)), "{missing:?}");
}
