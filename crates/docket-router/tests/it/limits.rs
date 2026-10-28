//! "Always ask" and "never": a limit outranks every allowance the person gave, a "never" answer
//! refuses and is kept, the assistant can only tighten, and the administrator's floor binds
//! where the person's files say otherwise.

use crate::support::*;
use docket_core::*;
use docket_fake::ScriptedConfirmer;
use docket_permissions_core::{
    AlwaysOffer, AskReason, COMPANION_APP, DenyCode, Floor, FloorEntry, FloorFile, Limit,
    LimitCaller, LimitKey, LimitLevel, LimitMaker, LimitScope, Revocation, UndoOffer, rule_ids,
};
use docket_router::GrantStore;
use docket_vocab::{CallId, CallRequest, ChoiceId, Origin, TargetValue, Value};
use prov::{Actor, AgentRole, Effect, Labelled, SpaceScope};

fn archive() -> CallRequest {
    call("mail.thread.archive", &["t1"], vec![])
}

fn on_mail(level: LimitLevel, by: LimitMaker) -> Limit {
    Limit::new(
        LimitKey {
            caller: LimitCaller::AnyAgent,
            scope: LimitScope::App(mail_app()),
            space: SpaceScope::Any,
            from: Effect::Read,
            level,
        },
        by,
        None,
        prov::UnixSeconds(1),
    )
}

#[tokio::test]
async fn always_ask_beats_the_persons_always_allow_and_says_why() {
    let router = router();
    ready(&router).await;
    perform(&router, archive()).await.expect("allowed outright");
    assert!(router.seams.confirmer.requests().is_empty());
    let ask = on_mail(LimitLevel::AlwaysAsk, LimitMaker::Person);
    router.seams.grants.add_limit(ask.clone());
    perform(&router, archive())
        .await
        .expect_err("asks, then dismissed");
    let shown = router.seams.confirmer.requests();
    assert_eq!(shown.len(), 1);
    assert!(shown[0].why.contains(&AskReason::AskAlways));
    assert_eq!(shown[0].limits.held, Some(ask.note()));
    assert_eq!(shown[0].limits.never, NeverOffer::Offered);
    assert_eq!(shown[0].offer, ConfirmOffer::OnceOnly);
    assert!(matches!(shown[0].always, AlwaysOffer::Withheld(_)));
    // Undo is the one revoke path. Another thread, since the same call a third time is the
    // repeat guard's.
    assert_eq!(router.revoke_standing(&ask.id), Revocation::Revoked);
    perform(&router, call("mail.thread.archive", &["t2"], vec![]))
        .await
        .expect("allowed again");
}

#[tokio::test]
async fn a_never_answer_refuses_and_is_kept() {
    let mut router = router();
    ready(&router).await;
    router
        .seams
        .grants
        .add_limit(on_mail(LimitLevel::AlwaysAsk, LimitMaker::Assistant));
    router.seams.confirmer =
        ScriptedConfirmer::answering(vec![ConfirmAnswer::Never { receipt: receipt() }]);
    let refused = perform(&router, archive()).await.expect_err("never");
    assert_eq!(refused, CallRefusal::Unconfirmed(ConfirmEnd::Refused));
    // The person's "never" is a rule in the person's file, not in the assistant's.
    let written = router.seams.grants.rule_texts();
    assert_eq!(rule_ids(&written.never).len(), 1, "{}", written.never);
    assert!(written.assistant.is_empty(), "{}", written.assistant);
    assert!(router.seams.sink.records().iter().any(|r| matches!(
        r,
        AuditRecord::Confirm {
            answer: ConfirmAnswerKind::Never,
            input: Some(_),
            ..
        }
    )));
    // Next time, for any thing it acts on, it is refused without asking, and says so.
    router.seams.confirmer.clear();
    let other = call("mail.thread.archive", &["t2"], vec![]);
    let again = perform(&router, other).await.expect_err("refused");
    assert_eq!(again, CallRefusal::Denied(DenyCode::Never));
    assert!(router.seams.confirmer.requests().is_empty());
}

#[tokio::test]
async fn a_locked_floor_entry_beats_the_persons_always_allow() {
    let mut router = router();
    ready(&router).await;
    router.floor = Floor::from_file(FloorFile {
        strictness: None,
        locked: vec![FloorEntry {
            level: LimitLevel::Never,
            app: Some(mail_app()),
            action: Some(action("mail.thread.archive").name),
            from: Effect::Read,
        }],
        rows: Vec::new(),
    })
    .expect("floor");
    let refused = perform(&router, archive()).await.expect_err("locked");
    assert_eq!(refused, CallRefusal::Denied(DenyCode::Never));
    // The person cannot remove what the floor holds.
    let locked = router.floor.locked()[0].id.clone();
    assert_eq!(router.revoke_standing(&locked), Revocation::NotHeld);
    // The person's own launcher acts never meet it.
    let launched = ask(
        &router,
        &launcher(),
        IntentsRequest::Perform {
            activation: None,
            call: archive(),
            session: None,
            parent_window: None,
        },
    )
    .await;
    match launched {
        IntentsReply::Performed(result) => assert!(
            !matches!(*result, Err(CallRefusal::Denied(DenyCode::Never))),
            "the floor is for agents"
        ),
        other => panic!("perform: {other:?}"),
    }
}

fn tighten(
    router: &docket_router::Router<docket_fake::FakeSeams>,
    session: &prov::SessionId,
    pairs: &[(&str, Value)],
) -> Result<Outcome, AppRefusal> {
    router.companion_perform(Invocation {
        call: CallId(1),
        action: prov::ActionName::parse("companion.limit.add").expect("action"),
        target: TargetValue::Nothing,
        args: pairs
            .iter()
            .map(|(name, value)| {
                (
                    param(name),
                    Labelled {
                        value: value.clone(),
                        label: trusted(),
                    },
                )
            })
            .collect(),
        actor: Actor::Companion {
            session: session.clone(),
            role: AgentRole::Planner,
        },
        origin: Origin::Companion,
        space: space("work"),
    })
}

fn choice(text: &str) -> Value {
    Value::Choice(ChoiceId::parse(text).expect("choice"))
}

#[tokio::test]
async fn the_assistant_can_tighten_but_never_loosen() {
    let router = router();
    let opened = ready(&router).await;
    assert_eq!(COMPANION_APP, "org.quire.Companion");
    // "allow" is not a level the action reads.
    let loosened = tighten(&router, &opened.session, &[("level", choice("allow"))]);
    assert!(loosened.is_err(), "{loosened:?}");
    assert!(router.seams.grants.limits().is_empty());
    assert!(router.seams.grants.rule_texts().assistant.is_empty());
    // A quote the person did not say is dropped; one they said is kept.
    tighten(
        &router,
        &opened.session,
        &[
            ("level", choice("never")),
            ("app", Value::Text("org.quire.Mail".into())),
            ("said", Value::Text("something nobody said".into())),
        ],
    )
    .expect("tightened");
    // A forbid in the assistant's file, for this Space, in plain words; nothing in the person's.
    let written = router.seams.grants.rule_texts();
    assert_eq!(
        rule_ids(&written.assistant).len(),
        1,
        "{}",
        written.assistant
    );
    assert!(
        written.assistant.contains("forbid"),
        "{}",
        written.assistant
    );
    assert!(written.assistant.contains("context.space == \"work\""));
    assert!(written.assistant.contains("You said never to do this."));
    assert!(!written.assistant.contains("something nobody said"));
    assert!(written.never.is_empty());
    let refused = perform(&router, archive()).await.expect_err("never");
    assert_eq!(refused, CallRefusal::Denied(DenyCode::Never));
    router.seams.grants.clear();
    tighten(
        &router,
        &opened.session,
        &[
            ("level", choice("ask")),
            ("said", Value::Text("tidy my inbox".into())),
        ],
    )
    .expect("tightened");
    let kept = router.seams.grants.rule_texts().assistant;
    assert!(kept.contains("tidy my inbox"), "{kept}");
    assert!(kept.contains("perform_unasked"), "{kept}");
}

#[tokio::test]
async fn an_ask_first_rule_makes_the_next_call_ask_and_removing_it_by_id_restores_the_old_way() {
    let router = router();
    let opened = ready(&router).await;
    tighten(
        &router,
        &opened.session,
        &[
            ("level", choice("ask")),
            ("app", Value::Text("org.quire.Mail".into())),
        ],
    )
    .expect("tightened");
    let ids = rule_ids(&router.seams.grants.rule_texts().assistant);
    assert_eq!(ids.len(), 1);
    perform(&router, archive())
        .await
        .expect_err("asks, then dismissed");
    let shown = router.seams.confirmer.requests();
    assert_eq!(shown.len(), 1, "the rule made it ask");
    let held = shown[0]
        .limits
        .held
        .as_ref()
        .expect("the sheet names the rule");
    assert_eq!(held.id, ids[0]);
    assert_eq!(held.undo, UndoOffer::Offered);
    assert!(matches!(shown[0].always, AlwaysOffer::Withheld(_)));
    // Undo is the one revoke path: the rule leaves the file and the next call runs unasked.
    assert_eq!(router.revoke_standing(&ids[0]), Revocation::Revoked);
    assert!(rule_ids(&router.seams.grants.rule_texts().assistant).is_empty());
    perform(&router, call("mail.thread.archive", &["t2"], vec![]))
        .await
        .expect("allowed again");
    assert_eq!(router.seams.confirmer.requests().len(), 1);
}
