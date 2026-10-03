//! The terminal's other surfaces: a dry run goes through the same gate, undo and the journal are
//! the terminal's own rows only, a terminal reaches nothing that is not its to use, and the
//! control centre lists and revokes the standing grants.

mod support;

use docket_core::*;
use docket_fake::ScriptedConfirmer;
use docket_router::Router;
use prov::{Actor, UnixSeconds};
use support::*;

fn answering(router: &mut Router<docket_fake::FakeSeams>, answers: Vec<ConfirmAnswer>) {
    router.seams.confirmer = ScriptedConfirmer::answering(answers);
}

fn from_terminal() -> ConfirmAnswer {
    ConfirmAnswer::AllowedFromTerminal { receipt: receipt() }
}

fn once() -> ConfirmAnswer {
    ConfirmAnswer::Allowed {
        scope: GrantScope::Once,
        receipt: receipt(),
    }
}

fn shown(router: &Router<docket_fake::FakeSeams>) -> Vec<ConfirmRequest> {
    router.seams.confirmer.requests()
}

async fn run(
    router: &Router<docket_fake::FakeSeams>,
    request: CallRequest,
) -> Result<Outcome, CallRefusal> {
    match ask(
        router,
        &cli(),
        IntentsRequest::Perform {
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

fn archive() -> CallRequest {
    from_cli("mail.thread.archive", &["t2"], vec![])
}

#[tokio::test]
async fn a_dry_run_goes_through_the_same_gate_and_asks_nobody() {
    let router = router();
    let dry = |call: CallRequest| async {
        ask(
            &router,
            &cli(),
            IntentsRequest::DryRun {
                call,
                session: None,
            },
        )
        .await
    };
    let contact = Value::Entity(prov::EntityId {
        app: mail_app(),
        kind: prov::EntityKind::parse("mail.contact").expect("kind"),
        key: prov::EntityKey::parse("c1").expect("key"),
    });
    let preview = dry(from_cli(
        "mail.message.forward",
        &["t2"],
        vec![("to", contact.clone())],
    ))
    .await;
    assert!(
        matches!(preview, IntentsReply::Preview(Preview::Message { .. })),
        "{preview:?}"
    );
    assert!(shown(&router).is_empty(), "a dry run asks nobody");
    assert!(
        router.seams.link.mail.sent().is_empty(),
        "and sends nothing"
    );
    // No preview declared: still checked, nothing to show.
    let none = dry(from_cli("mail.thread.archive", &["t2"], vec![])).await;
    assert_eq!(none, IntentsReply::Preview(Preview::None));
    // A bad argument is refused exactly as the call would be.
    let bad = dry(from_cli("mail.message.forward", &["t2"], vec![])).await;
    assert!(
        matches!(
            bad,
            IntentsReply::Refused(WireRefusal::Call(CallRefusal::BadArgs { .. }))
        ),
        "{bad:?}"
    );
    // A hidden action is refused, not previewed.
    let hidden = dry(CallRequest {
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
    })
    .await;
    assert_eq!(
        hidden,
        IntentsReply::Refused(WireRefusal::Call(CallRefusal::Denied(DenyCode::NotAllowed)))
    );
}

#[tokio::test]
async fn a_terminal_undoes_only_what_a_terminal_did() {
    let mut router = router();
    answering(&mut router, vec![once()]);
    let done = run(&router, archive()).await.expect("archived");
    assert!(matches!(done.undo, Undoable::Journaled(_)));
    // The companion archives another thread.
    let _opened = ready(&router).await;
    perform(&router, call("mail.thread.archive", &["t1"], vec![]))
        .await
        .expect("archived by the companion");
    let mine = match ask(
        &router,
        &cli(),
        IntentsRequest::ControlJournal(JournalFilter {
            run: None,
            session: None,
            limit: porter_core::Count(10),
        }),
    )
    .await
    {
        IntentsReply::Journal(rows) => rows,
        other => panic!("{other:?}"),
    };
    assert_eq!(mine.len(), 1, "a terminal sees only its own rows");
    assert_eq!(mine[0].actor, Actor::Cli);
    let theirs = match ask(
        &router,
        &control(),
        IntentsRequest::ControlJournal(JournalFilter {
            run: None,
            session: None,
            limit: porter_core::Count(10),
        }),
    )
    .await
    {
        IntentsReply::Journal(rows) => rows,
        other => panic!("{other:?}"),
    };
    assert_eq!(theirs.len(), 2);
    let companions = theirs
        .iter()
        .find(|e| e.actor != Actor::Cli)
        .expect("the companion's row");
    assert_eq!(
        ask(&router, &cli(), IntentsRequest::Undo(companions.id)).await,
        IntentsReply::Refused(WireRefusal::NotAllowed)
    );
    assert_eq!(
        ask(&router, &cli(), IntentsRequest::Undo(mine[0].id)).await,
        IntentsReply::Undone(Ok(()))
    );
}

#[tokio::test]
async fn a_terminal_cannot_use_what_is_not_its_to_use() {
    let router = router();
    for request in [
        IntentsRequest::ControlHalt {
            scope: prov::SpaceScope::Any,
            cause: HaltCause::ControlCentre,
        },
        IntentsRequest::ControlResume {
            scope: prov::SpaceScope::Any,
        },
        IntentsRequest::ControlTerminalGrants,
        IntentsRequest::ControlTerminalRevoke(action("mail.thread.archive")),
        IntentsRequest::SessionTurn {
            session: prov::SessionId::parse("s-1").expect("id"),
            turn: TurnIn {
                text: "yes".into(),
                origin: Origin::Cli,
                keep: ContextKeep {
                    query: Keep::Dropped,
                    results: Keep::Dropped,
                    selection: Keep::Dropped,
                    window: Keep::Dropped,
                },
                via: TurnVia::Typed,
            },
        },
    ] {
        assert_eq!(
            ask(&router, &cli(), request.clone()).await,
            IntentsReply::Refused(WireRefusal::NotAllowed),
            "{request:?}"
        );
    }
}

#[tokio::test]
async fn the_control_centre_lists_and_revokes_the_grants() {
    let mut router = router();
    answering(&mut router, vec![from_terminal()]);
    assert!(run(&router, archive()).await.is_ok());
    assert_eq!(
        ask(&router, &control(), IntentsRequest::ControlTerminalGrants).await,
        IntentsReply::TerminalGrants(vec![action("mail.thread.archive")])
    );
    assert_eq!(
        ask(
            &router,
            &control(),
            IntentsRequest::ControlTerminalRevoke(action("mail.thread.archive"))
        )
        .await,
        IntentsReply::Done
    );
    assert_eq!(
        ask(&router, &control(), IntentsRequest::ControlTerminalGrants).await,
        IntentsReply::TerminalGrants(vec![])
    );
}

#[tokio::test]
async fn a_denial_the_person_recorded_for_the_terminal_stands() {
    use docket_router::GrantStore;
    use porter_core::consent::{Decision, Grant, Usage};
    let router = router();
    for (n, class) in [prov::DataClass::Mail].into_iter().enumerate() {
        router.seams.grants.record(Grant {
            id: porter_core::GrantId::parse(&format!("g-deny-{n}")).expect("grant"),
            key: ActionGrantKey {
                caller: GrantCaller::Cli,
                owner: mail_app(),
                target: GrantTarget::App,
                class,
                usage: Usage::Interactive,
                space: prov::SpaceScope::Any,
            },
            decision: Decision::Deny,
            scope: GrantScope::Always,
            at: UnixSeconds(0),
        });
    }
    assert_eq!(
        run(&router, from_cli("mail.thread.read", &["t2"], vec![])).await,
        Err(CallRefusal::Denied(DenyCode::NotAllowed))
    );
}
