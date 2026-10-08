//! A task policy's `paths` bound an agent's file writes: inside the paths the grid decides as it
//! does for any covered call, outside them the person is asked ("allow more for this task").

use crate::acp_agent::{CLAUDE, wide, world, write};
use docket_core::*;
use docket_fake::ScriptedConfirmer;
use prov::SessionId;

fn under(path: &str) -> TrustedPattern {
    TrustedPattern::Under(FileRef::parse(path).expect("file"))
}

fn bound_to_tests(w: &crate::acp_agent::World, session: &SessionId) {
    let task = w.router.state.lock().expect("lock").sessions[session]
        .task
        .clone();
    let mut policy = wide(&task);
    policy.paths = vec![under("/home/u/proj/tests")];
    crate::support::give_policy(&w.router, session, policy);
}

#[tokio::test]
async fn a_write_outside_the_policy_paths_asks_and_one_inside_runs() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::answering(vec![]);
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    bound_to_tests(&w, &session);

    w.call(&session, write("/home/u/proj/tests/a.rs"))
        .await
        .expect("inside the paths: the grid lets a covered write run");
    assert!(w.sheets().is_empty(), "{:?}", w.sheets());

    // The grid sends an outside, untrusted-free write to the reviewers under Default strictness
    // and asks under Ask more: the sheet below is that cell, reached because the path left the task.
    w.router.apply_settings(AgentConfig {
        strictness: Strictness::AskMore,
        ..AgentConfig::default()
    });

    w.call(&session, write("/home/u/proj/src/a.rs"))
        .await
        .expect_err("outside the paths: asked, and nobody answers");
    let asked = w.sheets();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].why.contains(&AskReason::OutsideTask), "{asked:?}");
    assert!(
        matches!(
            asked[0].always,
            AlwaysOffer::Withheld(Withheld::OutsideTask)
        ),
        "no always for a write outside the task"
    );
    assert_eq!(
        w.performed(),
        ["acpagent.files.write"],
        "the outside write never ran"
    );

    w.call(&session, write("/home/u/proj/tests-old/a.rs"))
        .await
        .expect_err("a sibling with the same prefix is outside too");
    assert_eq!(w.sheets().len(), 2);
}
