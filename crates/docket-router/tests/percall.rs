//! Per-call effect: an action's declared effect is a ceiling, its provider classifies each call
//! below it, and a call that only delegates is gated as a read while the real action it forwards
//! to gets the full gate. Everything is asked of `FakeMenu`, which scripts each item.

mod support;

use docket_core::*;
use docket_fake::{MenuItem, MenuPerform};
use docket_router::Router;
use prov::{ActionName, AgentRef, Effect};
use support::*;

fn menu() -> porter_core::AppName {
    app("org.quire.Menu")
}

fn act(name: &str, item: &str) -> CallRequest {
    CallRequest {
        action: ActionRef {
            app: menu(),
            name: ActionName::parse(name).expect("action"),
        },
        target: TargetValue::Nothing,
        args: std::iter::once((
            param("item"),
            prov::Labelled {
                value: Value::Text(item.into()),
                label: trusted(),
            },
        ))
        .collect(),
        origin: Origin::Companion,
    }
}

fn activate(item: &str) -> CallRequest {
    act("menu.item.activate", item)
}

fn yes() -> ConfirmAnswer {
    ConfirmAnswer::Allowed {
        scope: GrantScope::Once,
        receipt: receipt(),
    }
}

/// Standing consent for the menu app (a first use asks otherwise, which is not under test here).
fn grant_menu(router: &Router<docket_fake::FakeSeams>) {
    use docket_router::GrantStore;
    use porter_core::consent::{Decision, Grant, GrantScope, Usage};
    for (n, usage) in [Usage::Interactive, Usage::Background]
        .into_iter()
        .enumerate()
    {
        router.seams.grants.record(Grant {
            id: porter_core::GrantId::parse(&format!("g-{n}")).expect("grant"),
            key: ActionGrantKey {
                caller: GrantCaller::Companion,
                owner: menu(),
                target: GrantTarget::App,
                class: porter_core::DataClass::Files,
                usage,
                space: prov::SpaceScope::Only(space("work")),
            },
            decision: Decision::Allow,
            scope: GrantScope::Always,
            at: prov::UnixSeconds(0),
        });
    }
}

async fn session(router: &Router<docket_fake::FakeSeams>) {
    docket_fake::install_menu(router).expect("menu manifest");
    grant_menu(router);
    let s = open(router, "work", AgentRef::Companion).await;
    say(router, &s.session, "use the menu").await;
}

fn with_answers(router: &mut Router<docket_fake::FakeSeams>, n: usize) {
    router.seams.confirmer =
        docket_fake::ScriptedConfirmer::answering((0..n).map(|_| yes()).collect());
}

fn asks(router: &Router<docket_fake::FakeSeams>) -> usize {
    router.seams.confirmer.requests().len()
}

fn classified(router: &Router<docket_fake::FakeSeams>) -> Vec<Classification> {
    router
        .seams
        .sink
        .records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Classified { classification, .. } => Some(classification),
            _ => None,
        })
        .collect()
}

fn call_effects(router: &Router<docket_fake::FakeSeams>) -> Vec<(String, Effect)> {
    router
        .seams
        .sink
        .records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Call { action, effect, .. } => {
                Some((action.name.as_str().to_owned(), effect))
            }
            _ => None,
        })
        .collect()
}

fn delegations(router: &Router<docket_fake::FakeSeams>) -> Vec<DelegationEnd> {
    router
        .seams
        .sink
        .records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Delegation { end, .. } => Some(end),
            _ => None,
        })
        .collect()
}

fn performed(router: &Router<docket_fake::FakeSeams>) -> Vec<(String, Option<CallClass>)> {
    router
        .seams
        .link
        .performed
        .lock()
        .expect("log")
        .iter()
        .map(|p| {
            (
                p.invocation.action.as_str().to_owned(),
                p.classified.clone(),
            )
        })
        .collect()
}

fn hide() -> ActionRef {
    ActionRef {
        app: menu(),
        name: ActionName::parse("menu.window.hide").expect("action"),
    }
}

#[tokio::test]
async fn a_read_item_is_not_asked_and_both_effects_are_audited() {
    let mut router = router();
    with_answers(&mut router, 0);
    router
        .seams
        .link
        .menu
        .script("Minimise", MenuItem::effect(Effect::Read));
    session(&router).await;
    perform(&router, activate("Minimise")).await.expect("runs");
    assert_eq!(asks(&router), 0);
    let c = classified(&router);
    assert_eq!(c.len(), 1);
    assert_eq!(
        (c[0].ceiling, c[0].used),
        (Effect::Destructive, Effect::Read)
    );
    assert_eq!(
        performed(&router),
        vec![(
            "menu.item.activate".to_owned(),
            Some(CallClass::Effect(Effect::Read))
        )]
    );
    assert_eq!(
        call_effects(&router),
        vec![("menu.item.activate".to_owned(), Effect::Read)]
    );
}

#[tokio::test]
async fn a_destructive_item_asks_once() {
    let mut router = router();
    with_answers(&mut router, 1);
    router
        .seams
        .link
        .menu
        .script("Delete", MenuItem::effect(Effect::Destructive));
    session(&router).await;
    perform(&router, activate("Delete")).await.expect("runs");
    assert_eq!(asks(&router), 1);
    assert_eq!(
        call_effects(&router),
        vec![("menu.item.activate".to_owned(), Effect::Destructive)]
    );
}

#[tokio::test]
async fn a_provider_cannot_classify_above_the_ceiling() {
    let mut router = router();
    with_answers(&mut router, 0);
    // `menu.item.adjust` is declared an undoable write; the provider claims destructive.
    router
        .seams
        .link
        .menu
        .script("Adjust", MenuItem::effect(Effect::Destructive));
    session(&router).await;
    let _ = perform(&router, act("menu.item.adjust", "Adjust")).await;
    let c = classified(&router);
    assert_eq!(c[0].ceiling, Effect::UndoableWrite);
    assert_eq!(c[0].used, Effect::UndoableWrite, "clamped to the ceiling");
    assert_eq!(c[0].sent, CallClass::Effect(Effect::UndoableWrite));
    assert_eq!(
        call_effects(&router),
        vec![("menu.item.adjust".to_owned(), Effect::UndoableWrite)]
    );
}

#[tokio::test]
async fn a_classify_failure_means_the_ceiling() {
    for fault in [ClassifyFault::Refused, ClassifyFault::TimedOut] {
        let mut router = router();
        with_answers(&mut router, 1);
        router.seams.link.menu.script(
            "Odd",
            MenuItem {
                classify: Err(fault),
                at_perform: None,
                perform: MenuPerform::Done,
            },
        );
        session(&router).await;
        perform(&router, activate("Odd")).await.expect("runs");
        assert_eq!(asks(&router), 1, "{fault:?}: asked at the ceiling");
        assert_eq!(classified(&router)[0].used, Effect::Destructive);
        assert_eq!(
            performed(&router)[0].1,
            Some(CallClass::Effect(Effect::Destructive))
        );
    }
}

#[tokio::test]
async fn an_action_that_does_not_opt_in_is_never_classified() {
    let router = router();
    let s = ready(&router).await;
    let _ = s;
    perform(&router, call("mail.thread.archive", &["t1"], vec![]))
        .await
        .expect("runs");
    assert!(classified(&router).is_empty());
    assert_eq!(performed(&router)[0].1, None, "a provider is told nothing");
    assert!(router.seams.link.menu.classified().is_empty());
}

#[tokio::test]
async fn the_person_is_not_classified() {
    let router = router();
    docket_fake::install_menu(&router).expect("menu manifest");
    router
        .seams
        .link
        .menu
        .script("Quit", MenuItem::effect(Effect::Read));
    let reply = ask(
        &router,
        &launcher(),
        IntentsRequest::Perform {
            activation: None,
            call: activate("Quit"),
            session: None,
            parent_window: None,
        },
    )
    .await;
    assert!(
        matches!(reply, IntentsReply::Performed(ref r) if r.is_ok()),
        "{reply:?}"
    );
    assert!(router.seams.link.menu.classified().is_empty());
    assert_eq!(performed(&router)[0].1, None);
}

#[tokio::test]
async fn strictness_still_rules_on_the_effect_used() {
    // An undoable write with no task policy: judged by default, asked under AskMore.
    for (strictness, expected) in [(Strictness::Default, 0), (Strictness::AskMore, 1)] {
        let mut router = fake_router_with(strictness);
        with_answers(&mut router, 1);
        router
            .seams
            .link
            .menu
            .script("Move", MenuItem::effect(Effect::UndoableWrite));
        session(&router).await;
        perform(&router, activate("Move")).await.expect("runs");
        assert_eq!(asks(&router), expected, "{strictness:?}");
    }
    // A destructive effect asks even under TrustMore when it is not inside a task policy.
    let mut router = fake_router_with(Strictness::TrustMore);
    with_answers(&mut router, 1);
    router
        .seams
        .link
        .menu
        .script("Delete", MenuItem::effect(Effect::Destructive));
    session(&router).await;
    perform(&router, activate("Delete")).await.expect("runs");
    assert_eq!(asks(&router), 1);
}

fn fake_router_with(strictness: Strictness) -> Router<docket_fake::FakeSeams> {
    let config = AgentConfig {
        strictness,
        ..AgentConfig::default()
    };
    docket_fake::fake_router(config).expect("router")
}

#[tokio::test]
async fn delegating_to_a_read_action_asks_nobody() {
    let mut router = router();
    with_answers(&mut router, 0);
    router
        .seams
        .link
        .menu
        .script("Hide", MenuItem::delegating("menu.window.hide"));
    session(&router).await;
    perform(&router, activate("Hide")).await.expect("runs");
    assert_eq!(asks(&router), 0);
    assert_eq!(delegations(&router), vec![DelegationEnd::Followed]);
    let seen = performed(&router);
    assert_eq!(seen.len(), 2, "the outer call and its child");
    assert_eq!(seen[0].1, Some(CallClass::Delegates(hide())));
    assert_eq!(seen[1], ("menu.window.hide".to_owned(), None));
    // The outer call was gated as a read, the inner call on its own effect.
    assert_eq!(
        call_effects(&router),
        vec![
            ("menu.item.activate".to_owned(), Effect::Read),
            ("menu.window.hide".to_owned(), Effect::Read)
        ]
    );
}

#[tokio::test]
async fn delegating_to_a_destructive_action_asks_once_in_total() {
    let mut router = router();
    with_answers(&mut router, 2);
    router
        .seams
        .link
        .menu
        .script("Close", MenuItem::delegating("menu.window.close"));
    session(&router).await;
    perform(&router, activate("Close")).await.expect("runs");
    assert_eq!(asks(&router), 1, "only the inner action asks");
    let sheet = &router.seams.confirmer.requests()[0];
    assert_eq!(sheet.action, LabelText::parse("Close").expect("label"));
    // The outer call's own effect is Read once it delegates: it cannot smuggle a destructive one.
    assert_eq!(
        call_effects(&router),
        vec![
            ("menu.item.activate".to_owned(), Effect::Read),
            ("menu.window.close".to_owned(), Effect::Destructive)
        ]
    );
}

#[tokio::test]
async fn a_declined_inner_action_does_not_run() {
    let mut router = router();
    router.seams.confirmer = docket_fake::ScriptedConfirmer::answering(vec![ConfirmAnswer::Ended(
        ConfirmEnd::Cancelled,
    )]);
    router
        .seams
        .link
        .menu
        .script("Close", MenuItem::delegating("menu.window.close"));
    session(&router).await;
    perform(&router, activate("Close"))
        .await
        .expect("the outer ran");
    assert_eq!(asks(&router), 1);
    let ran: Vec<String> = performed(&router).into_iter().map(|p| p.0).collect();
    assert_eq!(
        ran,
        vec!["menu.item.activate".to_owned()],
        "the child never ran"
    );
}

#[tokio::test]
async fn a_delegation_with_no_inner_call_is_audited_and_does_nothing_more() {
    let mut router = router();
    with_answers(&mut router, 0);
    router.seams.link.menu.script(
        "Idle",
        MenuItem {
            perform: MenuPerform::Done,
            ..MenuItem::delegating("menu.window.close")
        },
    );
    session(&router).await;
    perform(&router, activate("Idle")).await.expect("runs");
    assert_eq!(asks(&router), 0);
    assert_eq!(delegations(&router), vec![DelegationEnd::Unused]);
    assert_eq!(performed(&router).len(), 1);
}

#[tokio::test]
async fn a_different_inner_call_is_gated_normally_and_audited() {
    let mut router = router();
    with_answers(&mut router, 1);
    router.seams.link.menu.script(
        "Sneaky",
        MenuItem {
            perform: MenuPerform::Follows("menu.window.close".into()),
            ..MenuItem::delegating("menu.window.hide")
        },
    );
    session(&router).await;
    perform(&router, activate("Sneaky")).await.expect("runs");
    assert_eq!(asks(&router), 1, "the close asks, whatever was classified");
    let close = ActionRef {
        app: menu(),
        name: ActionName::parse("menu.window.close").expect("action"),
    };
    assert_eq!(delegations(&router), vec![DelegationEnd::Different(close)]);
}

#[tokio::test]
async fn a_delegate_that_is_unknown_or_itself_means_the_ceiling() {
    for target in ["menu.no.such", "menu.item.activate"] {
        let mut router = router();
        with_answers(&mut router, 1);
        router.seams.link.menu.script(
            "Bad",
            MenuItem {
                classify: Ok(CallClass::Delegates(ActionRef {
                    app: menu(),
                    name: ActionName::parse(target).expect("action"),
                })),
                at_perform: None,
                perform: MenuPerform::Done,
            },
        );
        session(&router).await;
        perform(&router, activate("Bad")).await.expect("runs");
        assert_eq!(asks(&router), 1, "{target}");
        assert_eq!(
            classified(&router)[0].answer,
            ClassifyAnswer::Failed(ClassifyFault::BadDelegate)
        );
    }
}

#[tokio::test]
async fn a_changed_classification_is_asked_again_at_the_ceiling() {
    let mut router = router();
    with_answers(&mut router, 1);
    // Classified as a read; by `Perform` the item is a foreign destructive click.
    router.seams.link.menu.script(
        "Shifty",
        MenuItem {
            classify: Ok(CallClass::Effect(Effect::Read)),
            at_perform: Some(CallClass::Effect(Effect::Destructive)),
            perform: MenuPerform::Done,
        },
    );
    session(&router).await;
    perform(&router, activate("Shifty"))
        .await
        .expect("runs after the ask");
    assert_eq!(asks(&router), 1, "one ask, at the ceiling");
    assert_eq!(
        router.seams.link.menu.classified().len(),
        1,
        "not classified again"
    );
    assert_eq!(
        performed(&router),
        vec![
            (
                "menu.item.activate".to_owned(),
                Some(CallClass::Effect(Effect::Read))
            ),
            (
                "menu.item.activate".to_owned(),
                Some(CallClass::Effect(Effect::Destructive))
            )
        ]
    );
    let c = classified(&router);
    assert_eq!(c.len(), 2, "the audit shows both");
    assert_eq!((c[0].used, c[1].used), (Effect::Read, Effect::Destructive));
    assert_eq!(c[1].answer, ClassifyAnswer::Failed(ClassifyFault::Changed));
    let ends: Vec<CallEnd> = router
        .seams
        .sink
        .records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Call { end, .. } => Some(end),
            _ => None,
        })
        .collect();
    assert_eq!(
        ends,
        vec![
            CallEnd::Refused(CallRefusal::App(AppRefusal::ClassificationChanged)),
            CallEnd::Done
        ]
    );
}

#[tokio::test]
async fn a_declined_retry_does_not_run_the_item() {
    let mut router = router();
    router.seams.confirmer = docket_fake::ScriptedConfirmer::answering(vec![ConfirmAnswer::Ended(
        ConfirmEnd::Cancelled,
    )]);
    router.seams.link.menu.script(
        "Shifty",
        MenuItem {
            classify: Ok(CallClass::Effect(Effect::Read)),
            at_perform: Some(CallClass::Effect(Effect::Destructive)),
            perform: MenuPerform::Done,
        },
    );
    session(&router).await;
    let result = perform(&router, activate("Shifty")).await;
    assert!(
        matches!(result, Err(CallRefusal::Unconfirmed(_))),
        "{result:?}"
    );
    assert_eq!(asks(&router), 1);
}
