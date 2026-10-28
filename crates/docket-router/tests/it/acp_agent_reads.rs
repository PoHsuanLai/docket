//! An external agent's reads under Auto run unasked, in every app and for every program. A read
//! grant (the scope of a read-only action) stands only for a read, is never offered on a write or a
//! destructive action, and only the person's answer creates a grant.

use crate::acp_agent::*;
use crate::support::*;
use docket_core::*;
use docket_vocab::{ActionRef, CallRequest, TargetValue};
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
async fn an_external_agents_reads_run_unasked_in_every_app_and_leave_no_grant() {
    // Under Auto a read never asks, for any caller: connecting the program is the permission.
    let w = world();
    let claude = open_wide(&w, CLAUDE).await;
    w.call(&claude, call("mail.thread.read", &["t2"], vec![]))
        .await
        .expect("a mail read runs");
    // The fake Files app holds no such file: the read got past the gate and the app answered.
    let files = w.call(&claude, files_read()).await;
    assert!(
        matches!(files, Ok(_) | Err(CallRefusal::App(_))),
        "{files:?}"
    );
    let gemini = open_wide(&w, "gemini-cli").await;
    w.call(&gemini, call("mail.thread.read", &["t1"], vec![]))
        .await
        .expect("another program's read runs too");
    assert!(w.sheets().is_empty(), "{:?}", w.sheets());
    assert!(grants_of(&w).is_empty());
    assert_eq!(w.used(), 0);
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
async fn the_agent_cannot_create_a_grant_only_the_persons_answer_does() {
    let w = world();
    let claude = open_wide(&w, CLAUDE).await;
    // No answer: the sheet is dismissed, nothing is held.
    w.call(&claude, call("mail.thread.archive", &["t2"], vec![]))
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
