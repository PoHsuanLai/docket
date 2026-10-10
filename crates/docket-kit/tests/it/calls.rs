//! A built agent's calls reach the router, and only the router.

use crate::support::*;
use docket_core::{CallRefusal, StepEnd};
use docket_kit::{Ended, Limits};
use docket_tasks::Failure;
use porter_core::Count;
use serde_json::json;

fn archive() -> Say {
    call(ARCHIVE, json!({ "target": [thread("t2")] }))
}

#[tokio::test]
async fn a_tool_call_reaches_the_router_as_a_call_request() {
    let world = World::new(vec![archive(), words("Archived.")]);
    let agent = mail_agent(&world).await.build().expect("agent");
    let run = agent
        .ask(&asker(), "Archive the digest.")
        .await
        .expect("ask");
    assert_eq!(run.ended, Ended::Done);
    assert_eq!(
        world.performed(),
        1,
        "the router passed the call to the app"
    );
    assert_eq!(run.steps.len(), 1);
    assert!(matches!(run.steps[0].end, StepEnd::Done { .. }));
    assert_eq!(run.said, vec!["Archived.".to_owned()]);
}

/// Without standing consent the router asks on a sheet nobody answers: the agent is told the
/// call was not confirmed, and the app was never reached.
#[tokio::test]
async fn a_call_the_router_will_not_make_comes_back_without_the_app_being_reached() {
    let world = World::asking(vec![archive(), words("I could not archive it.")]);
    let agent = mail_agent(&world).await.build().expect("agent");
    let run = agent
        .ask(&asker(), "Archive the digest.")
        .await
        .expect("ask");
    assert_eq!(world.performed(), 0);
    assert!(
        matches!(run.steps[0].end, StepEnd::Unconfirmed(_)),
        "{:?}",
        run.steps
    );
}

/// A call to an action not in the set
/// chosen for the agent, so it never gets as far as the router.
#[tokio::test]
async fn a_call_outside_the_chosen_set_never_reaches_the_router() {
    let world = World::new(vec![archive(), words("Done.")]);
    let agent = mail_agent(&world)
        .await
        .actions(docket_kit::Actions::none())
        .build()
        .expect("agent");
    let run = agent
        .ask(&asker(), "Archive the digest.")
        .await
        .expect("ask");
    assert_eq!(world.performed(), 0);
    assert!(
        run.steps
            .iter()
            .all(|s| !matches!(s.end, StepEnd::Done { .. }))
    );
}

/// A delete in an app the task policy does not name does not go through: the router refuses it or
/// asks the person, whose sheet the fake dismisses. Either way the agent is told a coarse end and
/// nothing of the rule or the reviewer.
#[tokio::test]
async fn a_refusal_comes_back_as_a_coarse_code() {
    let delete = call(
        "org.quire.Files-files.file.delete",
        json!({ "target": [{ "app": "org.quire.Files", "kind": "files.file", "key": "f1" }] }),
    );
    let world = World::new(vec![delete, words("It was not allowed.")]);
    let link = link(&world);
    let actions = docket_kit::Actions::from(&catalogue(&link).await);
    let agent = docket_kit::Agent::builder(world.infer.clone(), link)
        .actions(actions)
        .build()
        .expect("agent");
    let run = agent.ask(&asker(), "Delete the file.").await.expect("ask");
    assert_eq!(world.performed(), 0, "the app was never reached");
    assert!(
        matches!(
            run.steps.first().map(|s| &s.end),
            Some(StepEnd::Refused(CallRefusal::Denied(_)) | StepEnd::Unconfirmed(_))
        ),
        "{:?}",
        run.steps
    );
    let told = world.infer.user_text(1);
    assert!(
        !told.contains("cedar") && !told.contains("policy"),
        "{told}"
    );
}

#[tokio::test]
async fn the_step_limit_ends_the_ask_failed() {
    let world = World::new(vec![archive(), words("Archived.")]);
    let agent = mail_agent(&world)
        .await
        .limits(Limits { steps: Count(1) })
        .build()
        .expect("agent");
    let run = agent
        .ask(&asker(), "Archive the digest.")
        .await
        .expect("ask");
    assert_eq!(run.ended, Ended::Failed);
    assert_eq!(run.failure, Some(Failure::Budget));
}

#[tokio::test]
async fn a_question_ends_the_ask_asking() {
    let world = World::new(vec![call(
        "quire_ask",
        json!({ "text": "Which one?", "choices": ["a", "b"] }),
    )]);
    let agent = mail_agent(&world).await.build().expect("agent");
    let run = agent.ask(&asker(), "Archive one.").await.expect("ask");
    assert_eq!(
        run.ended,
        Ended::Asked {
            text: "Which one?".into(),
            choices: vec!["a".into(), "b".into()]
        }
    );
}

/// A finish that names a handle the agent holds comes back in `Run::shown` (`ask` and
/// `ask_recorded` share the one driver that fills it); the
/// content itself is never in the run.
#[tokio::test]
async fn a_finish_that_shows_a_handle_hands_it_to_the_host() {
    let read = || {
        call(
            "org.quire.Mail-mail.thread.read",
            json!({ "target": { "app": "org.quire.Mail", "kind": "mail.thread", "key": "t1" } }),
        )
    };
    let finish = || call(docket_planner::TOOL_FINISH, json!({ "show": [1] }));
    let world = World::new(vec![read(), finish()]);
    let agent = mail_agent(&world).await.build().expect("agent");
    let run = agent
        .ask(&asker(), "Find the invoice and show me.")
        .await
        .expect("ask");
    assert_eq!(run.ended, Ended::Done);
    assert_eq!(run.shown, [docket_core::Handle(1)]);
    assert!(!format!("{run:?}").contains("IGNORE"), "a handle, not text");

    let world = World::new(vec![archive(), words("Archived.")]);
    let agent = mail_agent(&world).await.build().expect("agent");
    let run = agent
        .ask(&asker(), "Archive the digest.")
        .await
        .expect("ask");
    assert!(run.shown.is_empty(), "a finish that names none shows none");
}
