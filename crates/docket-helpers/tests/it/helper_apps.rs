//! Helpers that run an agent app, against a scripted host of agent apps: a board job is the app's
//! one turn, its words come back as the job's untrusted answer, an app that wants signing in makes
//! its helper wait and never fail a job, the app's own sign-in ways are the card's choices, and a
//! crash ends that job only. No process runs and no login exists.

use crate::support::apps::{
    Apps, done, give, hub, make, program, state_of, status_of, until, ways,
};
use crate::support::world::*;
use companion_wire::{HelperNeeds, HelperReply, JobShown, LoginShown};
use docket_core::helper::*;
use docket_tasks::HelperDesk;
use std::collections::BTreeMap;
use std::sync::Arc;

#[tokio::test]
async fn an_app_helper_takes_a_board_job_and_its_words_are_the_answer() {
    let w = world(vec![]);
    let apps = Arc::new(Apps::default());
    apps.will_run("Coder", vec![done("Three offers, the second is best.")]);
    let hub = hub(&w, &apps);
    // No model is chosen and none is needed: the app brings its own.
    let coder = make(&hub, "Coder").await;
    let job = give(&hub, &coder, "compare the three offers").await;

    until("the job is done", || {
        matches!(status_of(&hub, job), JobStatus::Done(_))
    })
    .await;
    assert_eq!(
        hub.answer_words(job).as_deref(),
        Some("Three offers, the second is best.")
    );
    // The goal is the whole of what the app was told; no model of ours was asked.
    assert_eq!(
        apps.goals.lock().expect("goals").clone(),
        vec![("Coder".to_owned(), "compare the three offers".to_owned())]
    );
    assert!(w.infer.asked().is_empty());
    assert_eq!(state_of(&hub, &coder), HelperState::Idle);
    assert_eq!(
        hub.card(&coder).expect("card").kind,
        HelperKind::AgentApp(program())
    );
}

#[tokio::test]
async fn an_app_that_wants_signing_in_makes_its_helper_wait_and_fails_no_job() {
    let w = world(vec![]);
    let apps = Arc::new(Apps::default());
    apps.will_run("Coder", vec![AgentOutcome::NeedsSignIn(ways())]);
    let hub = hub(&w, &apps);
    let coder = make(&hub, "Coder").await;
    let job = give(&hub, &coder, "compare the three offers").await;

    until("the helper waits", || {
        state_of(&hub, &coder) == HelperState::Waiting(Need::SignIn)
    })
    .await;
    assert_eq!(status_of(&hub, job), JobStatus::Blocked(Need::SignIn));
    assert_eq!(
        hub.board().job(job).expect("job").failed,
        None,
        "not a failed job"
    );

    // The list says what it needs; the person is told once, in plain words.
    let view = hub.view(0);
    assert_eq!(view.helpers[0].needs_you, Some(HelperNeeds::SignIn));
    assert_eq!(view.told.len(), 1);
    assert!(view.told[0].text.contains("sign in"), "{:?}", view.told);
    for word in ["ACP", "authenticate", "MCP"] {
        assert!(!view.told[0].text.contains(word), "{:?}", view.told);
    }

    // The card offers the app's own names for the ways.
    let card = hub.card(&coder).expect("card");
    assert_eq!(card.login, LoginShown::NeedsSignIn);
    assert_eq!(card.jobs[0].state, JobShown::NeedsSignIn);
    let names: Vec<_> = card.sign_in.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        ["Sign in with your Claude account", "Sign in in a terminal"]
    );
}

#[tokio::test]
async fn choosing_an_agent_way_signs_in_and_the_job_proceeds() {
    let w = world(vec![]);
    let apps = Arc::new(Apps::default());
    apps.will_run(
        "Coder",
        vec![
            AgentOutcome::NeedsSignIn(ways()),
            done("Signed in, and done."),
        ],
    );
    apps.sign_ins
        .lock()
        .expect("sign ins")
        .push_back(SignInOutcome::SignedIn);
    let hub = hub(&w, &apps);
    let coder = make(&hub, "Coder").await;
    let job = give(&hub, &coder, "compare the three offers").await;
    until("the helper waits", || {
        state_of(&hub, &coder) == HelperState::Waiting(Need::SignIn)
    })
    .await;

    let reply = hub.sign_in(coder.clone(), "claude-login".to_owned()).await;
    assert_eq!(reply, HelperReply::Done);
    assert_eq!(
        apps.signed_with.lock().expect("ways").clone(),
        ["claude-login"]
    );
    until("the job is done", || {
        matches!(status_of(&hub, job), JobStatus::Done(_))
    })
    .await;
    assert_eq!(hub.card(&coder).expect("card").login, LoginShown::SignedIn);

    // A way that is not on the card is not run.
    let refused = hub.sign_in(coder.clone(), "made-up".to_owned()).await;
    assert!(matches!(refused, HelperReply::Refused(_)), "{refused:?}");
}

#[tokio::test]
async fn a_terminal_way_asks_the_shell_to_open_one_and_resuming_asks_the_app_again() {
    let w = world(vec![]);
    let apps = Arc::new(Apps::default());
    apps.will_run(
        "Coder",
        vec![AgentOutcome::NeedsSignIn(ways()), done("Back to it.")],
    );
    let login = TerminalLogin {
        args: vec!["/login".to_owned()],
        env: BTreeMap::new(),
        run: None,
    };
    apps.sign_ins
        .lock()
        .expect("sign ins")
        .push_back(SignInOutcome::OpenTerminal(login.clone()));
    let hub = hub(&w, &apps);
    let coder = make(&hub, "Coder").await;
    let job = give(&hub, &coder, "compare the three offers").await;
    until("the helper waits", || {
        state_of(&hub, &coder) == HelperState::Waiting(Need::SignIn)
    })
    .await;

    let reply = hub
        .sign_in(coder.clone(), "terminal-login".to_owned())
        .await;
    assert_eq!(
        reply,
        HelperReply::OpenTerminal(TerminalRequest {
            helper: coder.clone(),
            program: program(),
            run: login.run.clone(),
            login,
            handoff: None,
        })
    );
    // Nothing is signed in yet and the job still waits.
    assert_eq!(status_of(&hub, job), JobStatus::Blocked(Need::SignIn));

    // The terminal closed: the person says try again, and the app is asked once more.
    hub.resume(coder.clone()).await;
    until("the job is done", || {
        matches!(status_of(&hub, job), JobStatus::Done(_))
    })
    .await;
}

#[tokio::test]
async fn one_login_serves_every_helper_that_runs_the_same_app() {
    let w = world(vec![]);
    let apps = Arc::new(Apps::default());
    apps.will_run(
        "Coder",
        vec![AgentOutcome::NeedsSignIn(ways()), done("One.")],
    );
    apps.will_run(
        "Reviewer",
        vec![AgentOutcome::NeedsSignIn(ways()), done("Two.")],
    );
    apps.sign_ins
        .lock()
        .expect("sign ins")
        .push_back(SignInOutcome::SignedIn);
    let hub = hub(&w, &apps);
    let coder = make(&hub, "Coder").await;
    let reviewer = make(&hub, "Reviewer").await;
    let first = give(&hub, &coder, "write it").await;
    let second = give(&hub, &reviewer, "read it").await;
    until("both wait", || {
        state_of(&hub, &coder) == HelperState::Waiting(Need::SignIn)
            && state_of(&hub, &reviewer) == HelperState::Waiting(Need::SignIn)
    })
    .await;

    // Signing in for one releases the other: the app has one login.
    assert_eq!(
        hub.sign_in(coder.clone(), "claude-login".to_owned()).await,
        HelperReply::Done
    );
    until("both jobs are done", || {
        matches!(status_of(&hub, first), JobStatus::Done(_))
            && matches!(status_of(&hub, second), JobStatus::Done(_))
    })
    .await;
}

#[tokio::test]
async fn a_crash_ends_that_job_failed_and_open_again_and_touches_no_other_helper() {
    let w = world(vec![]);
    let apps = Arc::new(Apps::default());
    apps.will_run("Coder", vec![AgentOutcome::Failed(JobFault::Crashed)]);
    apps.will_run("Reviewer", vec![done("Fine.")]);
    let hub = hub(&w, &apps);
    let coder = make(&hub, "Coder").await;
    let reviewer = make(&hub, "Reviewer").await;
    let broken = give(&hub, &coder, "write it").await;
    let fine = give(&hub, &reviewer, "read it").await;

    until("the other job is done", || {
        matches!(status_of(&hub, fine), JobStatus::Done(_))
    })
    .await;
    until("the crashed job is open again", || {
        let job = hub.board().job(broken).cloned().expect("job");
        job.status == JobStatus::Open && job.failed == Some(JobFault::Crashed)
    })
    .await;
    assert_eq!(state_of(&hub, &coder), HelperState::Idle);
    assert_eq!(hub.answer_words(fine).as_deref(), Some("Fine."));
}
