//! An external agent's grants and approvals through its host: an Always on a sheet is a grant for the
//! program in docket's store, used and audited, listed and revoked over Control; a reviewer's no still
//! refuses a call a grant let through; a string of refusals pauses the session; a permission the
//! person allowed approves the matching call once.

use crate::acp_agent::*;
use crate::support::*;
use docket_core::*;
use docket_fake::{ReviewMode, ScriptedConfirmer, ScriptedReviewer};

#[tokio::test]
async fn an_always_on_a_write_is_a_grant_for_the_program_used_audited_listed_and_revoked() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::answering(vec![always()]);
    let claude = w.open(CLAUDE, SheetSurface::Desktop).await;
    // Taint the session so the write asks.
    w.call(&claude, read("/home/u/proj/a.rs"))
        .await
        .expect("read");
    w.call(&claude, write("/home/u/proj/src/a.rs"))
        .await
        .expect("asked, always");
    let held = w.router.standing_grants();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].caller, GrantCaller::AcpAgent(program(CLAUDE)));
    assert_eq!(held[0].scope, scope_under(FILES_WRITE, "/home/u/proj/src"));
    // A matching later call runs on the grant: no sheet, audited.
    w.call(&claude, write("/home/u/proj/src/b.rs"))
        .await
        .expect("on the grant");
    assert_eq!(w.sheets().len(), 1);
    assert_eq!(w.used(), 1);
    // A sibling directory asks again.
    w.call(&claude, write("/home/u/proj/other/c.rs"))
        .await
        .expect_err("asks");
    assert_eq!(w.sheets().len(), 2);
    // Another program has no such grant.
    let gemini = w.open("gemini-cli", SheetSurface::Desktop).await;
    w.call(&gemini, read("/home/u/proj/a.rs"))
        .await
        .expect("read");
    w.call(&gemini, write("/home/u/proj/src/a.rs"))
        .await
        .expect_err("asks");
    assert_eq!(w.sheets().len(), 3);
    assert_eq!(w.used(), 1);
    // Settings lists it over Control and revokes it.
    let listed = ask(&w.router, &control(), IntentsRequest::ControlStandingGrants).await;
    assert_eq!(listed, IntentsReply::StandingGrants(held.clone()));
    let gone = ask(
        &w.router,
        &control(),
        IntentsRequest::ControlStandingRevoke(held[0].id.clone()),
    )
    .await;
    assert_eq!(gone, IntentsReply::Done);
    let again = w
        .call(&claude, write("/home/u/proj/src/d.rs"))
        .await
        .expect_err("asks again");
    assert_eq!(w.sheets().len(), 4, "{again:?}");
}

#[tokio::test]
async fn a_reviewer_deny_refuses_a_call_a_grant_let_through() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::answering(vec![always()]);
    let claude = w.open(CLAUDE, SheetSurface::Desktop).await;
    w.call(&claude, read("/home/u/proj/a.rs"))
        .await
        .expect("read");
    w.call(&claude, write("/home/u/proj/src/a.rs"))
        .await
        .expect("always");
    w.router.seams.reviewer = ScriptedReviewer::queued(
        vec![Ok(action_review::ReviewVerdict::Deny {
            why: action_review::ReviewReason {
                code: ReasonCode::OutsideRequest,
                text: ReasonText("no".into()),
            },
        })],
        ReviewMode::AlwaysAllow,
    );
    let before = w.performed().len();
    let refused = w
        .call(&claude, write("/home/u/proj/src/b.rs"))
        .await
        .expect_err("the reviewer said no");
    assert_eq!(refused, CallRefusal::Denied(DenyCode::NotAllowed));
    assert_eq!(w.performed().len(), before, "nothing was performed");
}

#[tokio::test]
async fn three_refusals_pause_the_agents_session_until_the_person_speaks() {
    let w = world();
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    for line in ["make a", "make b", "make c"] {
        let refused = w.call(&session, run(line)).await.expect_err("dismissed");
        assert!(
            matches!(refused, CallRefusal::Unconfirmed(_)),
            "{refused:?}"
        );
    }
    let paused = w
        .call(&session, read("/home/u/proj/a.rs"))
        .await
        .expect_err("paused");
    assert_eq!(paused, CallRefusal::Paused(BreakerTrip::Probing));
    say_as(&w.router, &host(), &session, "carry on").await;
    w.call(&session, read("/home/u/proj/a.rs"))
        .await
        .expect("the person spoke");
}

#[tokio::test]
async fn a_permission_the_person_allowed_covers_the_matching_write_once() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::answering(vec![once()]);
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    w.call(&session, read("/home/u/proj/a.rs"))
        .await
        .expect("read");
    w.call(&session, permission("edit", &["/home/u/proj/a.rs"]))
        .await
        .expect("asked once, allowed");
    assert_eq!(w.sheets().len(), 1);
    w.call(&session, write("/home/u/proj/a.rs"))
        .await
        .expect("on the yes");
    assert_eq!(w.sheets().len(), 1, "not asked twice");
    assert!(w.records().iter().any(|r| matches!(
        r,
        AuditRecord::ApprovalUsed { action, .. } if action.name.as_str() == FILES_WRITE
    )));
    // Once: the next write of the same file asks.
    w.call(&session, write("/home/u/proj/a.rs"))
        .await
        .expect_err("asks");
    assert_eq!(w.sheets().len(), 2);
}

#[tokio::test]
async fn a_permission_covers_only_what_it_named_and_ends_when_the_person_speaks() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::answering(vec![once()]);
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    w.call(&session, read("/home/u/proj/a.rs"))
        .await
        .expect("read");
    w.call(&session, permission("edit", &["/home/u/proj/a.rs"]))
        .await
        .expect("allowed");
    w.call(&session, write("/home/u/proj/b.rs"))
        .await
        .expect_err("another file asks");
    assert_eq!(w.sheets().len(), 2);
    say_as(&w.router, &host(), &session, "and now the other one").await;
    let again = w
        .call(&session, write("/home/u/proj/a.rs"))
        .await
        .expect_err("a new turn: asks");
    assert_eq!(w.sheets().len(), 3, "{again:?}");
}
