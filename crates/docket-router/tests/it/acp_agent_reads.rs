//! An external agent's "always" on a read-only action of an app's own: a standing grant for the
//! program and that one action, so a search or a read is asked about once. It never covers a write,
//! another action, another app or another program, and only the person's answer creates it.

use crate::acp_agent::*;
use crate::support::*;
use docket_core::*;
use docket_fake::ScriptedConfirmer;
use prov::{Effect, EntityId, EntityKey, EntityKind};
use std::collections::BTreeSet;

fn read_scope(app_action: &ActionRef) -> StandingScope {
    StandingScope::Reads {
        action: app_action.clone(),
    }
}

fn files_app() -> porter_core::AppName {
    porter_core::AppName::parse("org.quire.Files").expect("app")
}

pub(crate) fn files_read() -> CallRequest {
    let mut request = call("mail.thread.read", &[], vec![]);
    request.action = ActionRef {
        app: files_app(),
        name: prov::ActionName::parse("files.file.read").expect("action"),
    };
    request.target = TargetValue::Entities(vec![EntityId {
        app: files_app(),
        kind: EntityKind::parse("files.file").expect("kind"),
        key: EntityKey::parse("f1").expect("key"),
    }]);
    request
}

/// Opens an agent session whose task covers Mail and Files up to a destructive effect.
pub(crate) async fn open_wide(w: &World, program: &str) -> prov::SessionId {
    let session = w.open(program, SheetSurface::Desktop).await;
    let mut policy = {
        let st = w.router.state.lock().expect("lock");
        st.sessions
            .get(&session)
            .and_then(|r| r.policy.clone())
            .expect("policy")
    };
    policy.actions = BTreeSet::from([
        ActionMatch::AppUpTo(mail_app(), Effect::Destructive),
        ActionMatch::AppUpTo(files_app(), Effect::Destructive),
    ]);
    policy.kinds = BTreeSet::from([
        EntityKind::parse("mail.thread").expect("kind"),
        EntityKind::parse("files.file").expect("kind"),
    ]);
    give_policy(&w.router, &session, policy);
    session
}

pub(crate) fn grants_of(w: &World) -> Vec<StandingGrant> {
    w.router.standing_grants()
}

#[tokio::test]
async fn an_always_on_a_read_is_a_grant_for_that_action_and_the_next_read_asks_nothing() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::answering(vec![always()]);
    let claude = open_wide(&w, CLAUDE).await;
    w.call(&claude, call("mail.thread.read", &["t2"], vec![]))
        .await
        .expect("asked, always");
    let sheets = w.sheets();
    assert_eq!(sheets.len(), 1);
    let read = action("mail.thread.read");
    assert_eq!(sheets[0].always, AlwaysOffer::Offered(read_scope(&read)));
    let held = grants_of(&w);
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].caller, GrantCaller::AcpAgent(program(CLAUDE)));
    assert_eq!(held[0].scope, read_scope(&read));
    // Another thread, same action: quiet, audited.
    w.call(&claude, call("mail.thread.read", &["t1"], vec![]))
        .await
        .expect("on the grant");
    assert_eq!(w.sheets().len(), 1);
    assert_eq!(w.used(), 1);
}

#[tokio::test]
async fn the_grant_covers_no_other_program_app_or_action() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::answering(vec![always()]);
    let claude = open_wide(&w, CLAUDE).await;
    w.call(&claude, call("mail.thread.read", &["t2"], vec![]))
        .await
        .expect("always");
    // Another program asks.
    let gemini = open_wide(&w, "gemini-cli").await;
    w.call(&gemini, call("mail.thread.read", &["t2"], vec![]))
        .await
        .expect_err("another program asks");
    assert_eq!(w.sheets().len(), 2);
    // Another app's read asks.
    w.call(&claude, files_read())
        .await
        .expect_err("another app asks");
    assert_eq!(w.sheets().len(), 3);
    // A write of the same app asks, and its sheet offers no read grant.
    w.call(&claude, call("mail.thread.archive", &["t2"], vec![]))
        .await
        .expect_err("a write asks");
    let sheets = w.sheets();
    assert_eq!(sheets.len(), 4);
    assert!(
        !matches!(
            &sheets[3].always,
            AlwaysOffer::Offered(StandingScope::Reads { .. })
        ),
        "{:?}",
        sheets[3].always
    );
    assert_eq!(w.used(), 0);
    assert_eq!(grants_of(&w).len(), 1);
}

#[tokio::test]
async fn an_agent_is_offered_no_read_grant_on_a_write_or_a_destructive_action() {
    let w = world();
    let claude = open_wide(&w, CLAUDE).await;
    for request in [
        call("mail.thread.delete", &["t2"], vec![]),
        call("mail.thread.archive", &["t2"], vec![]),
    ] {
        w.call(&claude, request).await.expect_err("asks");
    }
    for sheet in w.sheets() {
        assert!(
            !matches!(
                sheet.always,
                AlwaysOffer::Offered(StandingScope::Reads { .. })
            ),
            "{:?}",
            sheet.always
        );
    }
    assert!(grants_of(&w).is_empty());
}

#[tokio::test]
async fn revoking_the_grant_makes_the_next_read_ask_again() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::answering(vec![always()]);
    let claude = open_wide(&w, CLAUDE).await;
    w.call(&claude, call("mail.thread.read", &["t2"], vec![]))
        .await
        .expect("always");
    let held = grants_of(&w);
    let listed = ask(&w.router, &control(), IntentsRequest::ControlStandingGrants).await;
    assert_eq!(listed, IntentsReply::StandingGrants(held.clone()));
    let gone = ask(
        &w.router,
        &control(),
        IntentsRequest::ControlStandingRevoke(held[0].id.clone()),
    )
    .await;
    assert_eq!(gone, IntentsReply::Done);
    w.call(&claude, call("mail.thread.read", &["t1"], vec![]))
        .await
        .expect_err("asks again");
    assert_eq!(w.sheets().len(), 2);
}

#[tokio::test]
async fn the_agent_cannot_create_the_grant_only_the_persons_answer_does() {
    let w = world();
    let claude = open_wide(&w, CLAUDE).await;
    // No answer: the sheet is dismissed, nothing is held.
    w.call(&claude, call("mail.thread.read", &["t2"], vec![]))
        .await
        .expect_err("dismissed");
    assert!(grants_of(&w).is_empty());
    // The host's role cannot reach the Control surface that lists and revokes grants.
    let reply = ask(&w.router, &host(), IntentsRequest::ControlStandingGrants).await;
    assert_eq!(reply, IntentsReply::Refused(WireRefusal::NotAllowed));
}

#[tokio::test]
async fn a_read_grant_stands_only_for_a_read() {
    let granted = StandingGrant::new(
        GrantCaller::AcpAgent(program(CLAUDE)),
        read_scope(&action("mail.thread.read")),
        prov::UnixSeconds(1),
    );
    let facts = CallFacts {
        action: action("mail.thread.read"),
        args: ArgFacts::Unscoped,
    };
    let held = [granted];
    let caller = GrantCaller::AcpAgent(program(CLAUDE));
    assert!(find_standing_for(&held, &caller, &facts, Effect::Read).is_some());
    for effect in [Effect::Outbound, Effect::UndoableWrite, Effect::Destructive] {
        assert!(find_standing_for(&held, &caller, &facts, effect).is_none());
    }
}
