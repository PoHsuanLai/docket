//! The terminal (`quire-do`, role `cli`): a read runs, everything else asks and never goes to a
//! reviewer, a hidden action is refused, arguments are untrusted from `Source::Cli`, and the one
//! way to skip the ask is the person's own "allow from the terminal until logout" on the sheet.

use crate::support::*;
use docket_core::*;
use docket_fake::ScriptedConfirmer;
use docket_router::{Revoked, Router};
use prov::{Actor, Source};

fn once() -> ConfirmAnswer {
    ConfirmAnswer::Allowed {
        scope: GrantScope::Once,
        receipt: receipt(),
    }
}

fn from_terminal() -> ConfirmAnswer {
    ConfirmAnswer::AllowedFromTerminal { receipt: receipt() }
}

async fn run(
    router: &Router<docket_fake::FakeSeams>,
    request: CallRequest,
) -> Result<Outcome, CallRefusal> {
    match ask(
        router,
        &cli(),
        IntentsRequest::Perform {
            activation: None,
            call: request,
            session: None,
            parent_window: None,
        },
    )
    .await
    {
        IntentsReply::Performed(result) => *result,
        other => panic!("perform: {other:?}"),
    }
}

fn answering(router: &mut Router<docket_fake::FakeSeams>, answers: Vec<ConfirmAnswer>) {
    router.seams.confirmer = ScriptedConfirmer::answering(answers);
}

fn shown(router: &Router<docket_fake::FakeSeams>) -> Vec<ConfirmRequest> {
    router.seams.confirmer.requests()
}

fn archive() -> CallRequest {
    from_cli("mail.thread.archive", &["t2"], vec![])
}

#[tokio::test]
async fn a_read_from_the_terminal_runs_without_asking() {
    let router = router();
    let read = run(&router, from_cli("mail.thread.read", &["t2"], vec![])).await;
    assert!(read.is_ok(), "{read:?}");
    assert!(shown(&router).is_empty(), "a read asks nobody");
}

#[tokio::test]
async fn an_undoable_write_asks_as_the_terminal_and_never_goes_to_a_reviewer() {
    let router = router();
    let end = run(&router, archive()).await;
    assert_eq!(end, Err(CallRefusal::Unconfirmed(ConfirmEnd::Dismissed)));
    let sheets = shown(&router);
    assert_eq!(sheets.len(), 1, "the person is asked");
    assert_eq!(sheets[0].actor, Actor::Cli);
    assert_eq!(sheets[0].why, [AskReason::FromTerminal]);
    assert_eq!(sheets[0].offer, ConfirmOffer::OnceOrFromTerminal);
    assert!(
        router.seams.reviewer.calls().is_empty(),
        "no auto-approval for a terminal"
    );
}

#[tokio::test]
async fn every_effect_asks_in_every_strictness_and_no_reviewer_runs() {
    for strictness in [
        Strictness::AskMore,
        Strictness::Default,
        Strictness::TrustMore,
    ] {
        for (name, targets, args) in [
            ("mail.thread.archive", vec!["t2"], vec![]),
            ("mail.thread.delete", vec!["t2"], vec![]),
            (
                "mail.draft.create",
                vec![],
                vec![("body", Value::Text("hi".into()))],
            ),
        ] {
            let router = router();
            router
                .state
                .lock()
                .expect("lock")
                .strictness
                .insert(space("work"), strictness);
            let end = run(&router, from_cli(name, &targets, args)).await;
            assert_eq!(
                end,
                Err(CallRefusal::Unconfirmed(ConfirmEnd::Dismissed)),
                "{name} {strictness:?}"
            );
            assert_eq!(shown(&router).len(), 1, "{name} {strictness:?}");
            assert!(
                router.seams.reviewer.calls().is_empty(),
                "{name} {strictness:?}"
            );
        }
    }
}

#[tokio::test]
async fn an_outbound_call_asks_and_its_untrusted_recipient_keeps_the_grant_off_the_sheet() {
    let router = router();
    let contact = Value::Entity(prov::EntityId {
        app: mail_app(),
        kind: prov::EntityKind::parse("mail.contact").expect("kind"),
        key: prov::EntityKey::parse("c1").expect("key"),
    });
    let end = run(
        &router,
        from_cli(
            "mail.message.send",
            &[],
            vec![("to", contact), ("body", Value::Text("hello".into()))],
        ),
    )
    .await;
    assert_eq!(end, Err(CallRefusal::Unconfirmed(ConfirmEnd::Dismissed)));
    let sheet = &shown(&router)[0];
    assert!(
        sheet.why.contains(&AskReason::FromTerminal),
        "{:?}",
        sheet.why
    );
    assert!(
        sheet
            .why
            .iter()
            .any(|r| matches!(r, AskReason::UntrustedSink(_))),
        "{:?}",
        sheet.why
    );
    assert_eq!(
        sheet.offer,
        ConfirmOffer::OnceOnly,
        "a recipient typed in a terminal is untrusted: no standing grant can cover it"
    );
    assert!(router.seams.link.mail.sent().is_empty());
}

#[tokio::test]
async fn arguments_from_the_terminal_are_untrusted_and_named_for_the_terminal() {
    let router = router();
    let end = run(
        &router,
        from_cli(
            "mail.draft.create",
            &[],
            vec![("body", Value::Text("hi".into()))],
        ),
    )
    .await;
    assert!(end.is_err());
    let sheet = &shown(&router)[0];
    let line = sheet.lines.first().expect("a line for the body");
    assert!(
        matches!(
            &line.value,
            Shown::Quoted {
                from: Source::Cli,
                ..
            }
        ),
        "{line:?}"
    );
}

#[tokio::test]
async fn allowing_once_runs_it_and_the_next_call_asks_again() {
    let mut router = router();
    answering(&mut router, vec![once(), once()]);
    assert!(run(&router, archive()).await.is_ok());
    assert!(
        run(&router, from_cli("mail.thread.archive", &["t1"], vec![]))
            .await
            .is_ok()
    );
    assert_eq!(shown(&router).len(), 2);
    assert!(router.terminal_grants().is_empty());
}

#[tokio::test]
async fn a_hidden_action_is_refused_without_asking() {
    let router = router();
    let forget = CallRequest {
        action: ActionRef {
            app: app("org.quire.Memory"),
            name: prov::ActionName::parse("memory.forget").expect("action"),
        },
        target: TargetValue::Entities(vec![prov::EntityId {
            app: app("org.quire.Memory"),
            kind: prov::EntityKind::parse("memory.fact").expect("kind"),
            key: prov::EntityKey::parse("f1").expect("key"),
        }]),
        args: Default::default(),
        origin: Origin::Cli,
    };
    assert_eq!(
        run(&router, forget).await,
        Err(CallRefusal::Denied(DenyCode::NotAllowed))
    );
    assert!(shown(&router).is_empty());
}

#[tokio::test]
async fn a_standing_grant_skips_the_ask_until_it_is_revoked() {
    let mut router = router();
    answering(&mut router, vec![from_terminal(), once()]);
    assert!(run(&router, archive()).await.is_ok(), "the person said yes");
    assert_eq!(router.terminal_grants(), [action("mail.thread.archive")]);
    // The same action again, and on another thread: no sheet.
    assert!(
        run(&router, from_cli("mail.thread.archive", &["t1"], vec![]))
            .await
            .is_ok()
    );
    assert_eq!(shown(&router).len(), 1, "the grant skipped the ask");
    assert!(router.seams.reviewer.calls().is_empty());
    // Another action is not covered.
    assert!(
        run(
            &router,
            from_cli(
                "mail.draft.create",
                &[],
                vec![("body", Value::Text("x".into()))]
            )
        )
        .await
        .is_ok()
    );
    assert_eq!(shown(&router).len(), 2, "another action still asks");
    // Revoked: asks again.
    assert_eq!(
        router.revoke_terminal_grant(&action("mail.thread.archive")),
        Revoked::Done
    );
    assert_eq!(
        router.revoke_terminal_grant(&action("mail.thread.archive")),
        Revoked::NotHeld
    );
    assert!(router.terminal_grants().is_empty());
    let after = run(&router, from_cli("mail.thread.archive", &["t2"], vec![])).await;
    assert_eq!(after, Err(CallRefusal::Unconfirmed(ConfirmEnd::Dismissed)));
    assert_eq!(shown(&router).len(), 3);
}

#[tokio::test]
async fn a_standing_grant_ends_with_the_session() {
    let mut router = router();
    answering(&mut router, vec![from_terminal()]);
    assert!(run(&router, archive()).await.is_ok());
    assert!(
        run(&router, from_cli("mail.thread.archive", &["t1"], vec![]))
            .await
            .is_ok()
    );
    assert_eq!(shown(&router).len(), 1);
    router.end_terminal_sessions();
    assert!(
        router.terminal_grants().is_empty(),
        "logout took the grant with it"
    );
    let after = run(&router, from_cli("mail.thread.archive", &["t2"], vec![])).await;
    assert_eq!(after, Err(CallRefusal::Unconfirmed(ConfirmEnd::Dismissed)));
    assert_eq!(shown(&router).len(), 2, "after logout it asks again");
}

#[tokio::test]
async fn a_standing_grant_never_covers_a_destructive_act_or_one_that_asks_every_time() {
    let mut router = router();
    answering(&mut router, vec![from_terminal()]);
    let delete = || from_cli("mail.thread.delete", &["t2"], vec![]);
    assert!(run(&router, delete()).await.is_ok());
    assert_eq!(
        shown(&router)[0].offer,
        ConfirmOffer::OnceOnly,
        "never offered for destructive"
    );
    assert!(
        router.terminal_grants().is_empty(),
        "and never recorded, whatever the sheet answered"
    );
    assert_eq!(
        run(&router, from_cli("mail.thread.delete", &["t1"], vec![])).await,
        Err(CallRefusal::Unconfirmed(ConfirmEnd::Dismissed))
    );
    let duplicate = CallRequest {
        action: ActionRef {
            app: app("org.quire.Files"),
            name: prov::ActionName::parse("files.file.duplicate").expect("action"),
        },
        target: TargetValue::Entities(vec![prov::EntityId {
            app: app("org.quire.Files"),
            kind: prov::EntityKind::parse("files.file").expect("kind"),
            key: prov::EntityKey::parse("f1").expect("key"),
        }]),
        args: Default::default(),
        origin: Origin::Cli,
    };
    let mut router = router;
    router
        .seams
        .link
        .files
        .add_file("f1", "/home/me/a.txt", "x");
    answering(&mut router, vec![]);
    let _ = run(&router, duplicate).await;
    let sheets = shown(&router);
    assert_eq!(sheets.len(), 1);
    assert_eq!(
        sheets[0].offer,
        ConfirmOffer::OnceOnly,
        "an ask-always action offers no standing grant"
    );
}

#[tokio::test]
async fn only_the_terminal_may_hold_the_grant() {
    // A sheet that answers "from the terminal" to a companion's call is still the person's yes
    // for that one call, and records nothing.
    let mut router = router();
    let opened = ready(&router).await;
    let _ = opened;
    answering(&mut router, vec![from_terminal()]);
    let planned = call(
        "mail.message.forward",
        &["t2"],
        vec![(
            "to",
            Value::Entity(prov::EntityId {
                app: mail_app(),
                kind: prov::EntityKind::parse("mail.contact").expect("kind"),
                key: prov::EntityKey::parse("c1").expect("key"),
            }),
        )],
    );
    let _ = perform(&router, planned).await;
    assert!(router.terminal_grants().is_empty());
}

fn forget(key: &str) -> CallRequest {
    CallRequest {
        action: ActionRef {
            app: app("org.quire.Memory"),
            name: prov::ActionName::parse("memory.forget").expect("action"),
        },
        target: TargetValue::Entities(vec![prov::EntityId {
            app: app("org.quire.Memory"),
            kind: prov::EntityKind::parse("memory.fact").expect("kind"),
            key: prov::EntityKey::parse(key).expect("key"),
        }]),
        args: Default::default(),
        origin: Origin::Cli,
    }
}

async fn resume_as(router: &Router<docket_fake::FakeSeams>, who: &CallerId) -> IntentsReply {
    ask(
        router,
        who,
        IntentsRequest::ControlResume {
            scope: prov::SpaceScope::Any,
        },
    )
    .await
}

/// Item 89: the breaker pauses a session "until the person speaks", and a terminal has no turn to
/// record. The way back is `Control.Resume` from the control centre, and nothing a terminal can
/// send does it.
#[tokio::test]
async fn a_terminal_the_breaker_paused_goes_on_only_when_the_control_centre_resumes() {
    let router = router();
    for key in ["f1", "f2", "f3"] {
        assert_eq!(
            run(&router, forget(key)).await,
            Err(CallRefusal::Denied(DenyCode::NotAllowed))
        );
    }
    let paused = run(&router, from_cli("mail.thread.read", &["t2"], vec![])).await;
    assert!(
        matches!(paused, Err(CallRefusal::Paused(_))),
        "three refusals in a row pause the terminal: {paused:?}"
    );

    // Neither the terminal itself nor any role but the control centre can resume it.
    assert_eq!(
        resume_as(&router, &cli()).await,
        IntentsReply::Refused(WireRefusal::NotAllowed)
    );
    assert_eq!(
        resume_as(&router, &launcher()).await,
        IntentsReply::Refused(WireRefusal::NotAllowed)
    );
    let still = run(&router, from_cli("mail.thread.read", &["t2"], vec![])).await;
    assert!(matches!(still, Err(CallRefusal::Paused(_))), "{still:?}");

    assert_eq!(resume_as(&router, &control()).await, IntentsReply::Done);
    let back = run(&router, from_cli("mail.thread.read", &["t2"], vec![])).await;
    assert!(back.is_ok(), "the control centre resumed it: {back:?}");

    // The breaker starts afresh: one refusal does not pause it again.
    assert_eq!(
        run(&router, forget("f4")).await,
        Err(CallRefusal::Denied(DenyCode::NotAllowed))
    );
    assert!(
        run(&router, from_cli("mail.thread.read", &["t2"], vec![]))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn resuming_one_space_leaves_a_terminal_paused_in_another() {
    let router = router();
    for key in ["f1", "f2", "f3"] {
        let _ = run(&router, forget(key)).await;
    }
    let other = ask(
        &router,
        &control(),
        IntentsRequest::ControlResume {
            scope: prov::SpaceScope::Only(prov::SpaceId::parse("work").expect("space")),
        },
    )
    .await;
    assert_eq!(other, IntentsReply::Done);
    let still = run(&router, from_cli("mail.thread.read", &["t2"], vec![])).await;
    assert!(
        matches!(still, Err(CallRefusal::Paused(_))),
        "the terminal lives in the desktop Space: {still:?}"
    );
    let own = ask(
        &router,
        &control(),
        IntentsRequest::ControlResume {
            scope: prov::SpaceScope::Only(prov::SpaceId::desktop()),
        },
    )
    .await;
    assert_eq!(own, IntentsReply::Done);
    assert!(
        run(&router, from_cli("mail.thread.read", &["t2"], vec![]))
            .await
            .is_ok()
    );
}
