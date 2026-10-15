//! "Always ask" and "never": a limit outranks every allowance the person gave, a "never" answer
//! refuses and is kept, the assistant can only tighten, and the administrator's floor binds
//! where the person's files say otherwise.

use crate::support::*;
use docket_core::*;
use docket_fake::ScriptedConfirmer;
use docket_router::{COMPANION_APP, GrantStore, Pending, tightened};
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

fn note() -> LimitNote {
    on_mail(LimitLevel::Never, LimitMaker::Person).note()
}

#[test]
fn a_limit_only_tightens_a_pending_step() {
    let confirm = || Pending::Confirm(vec![AskReason::FirstUse]);
    let refuse = || Pending::Refuse(CallRefusal::Halted(SpaceScope::Any));
    let rows = [
        (
            "free changes nothing",
            Pending::Run,
            Limited::Free,
            Pending::Run,
        ),
        (
            "ask turns a run into a question",
            Pending::Run,
            Limited::Ask(note()),
            Pending::Confirm(vec![AskReason::AskAlways]),
        ),
        (
            "ask turns a review into a question",
            Pending::NeedsReview(vec![Stage::Quick]),
            Limited::Ask(note()),
            Pending::Confirm(vec![AskReason::AskAlways]),
        ),
        (
            "ask adds its reason to a question already asked",
            confirm(),
            Limited::Ask(note()),
            Pending::Confirm(vec![AskReason::FirstUse, AskReason::AskAlways]),
        ),
        (
            "never refuses a run",
            Pending::Run,
            Limited::Never(note()),
            Pending::Refuse(CallRefusal::Denied(DenyCode::Never)),
        ),
        (
            "never refuses a question",
            confirm(),
            Limited::Never(note()),
            Pending::Refuse(CallRefusal::Denied(DenyCode::Never)),
        ),
        (
            "an earlier refusal stands",
            refuse(),
            Limited::Never(note()),
            refuse(),
        ),
    ];
    for (row, pending, limit, want) in rows {
        assert_eq!(tightened(pending, &limit), want, "{row}");
    }
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
    let kept: Vec<_> = router
        .seams
        .grants
        .limits()
        .into_iter()
        .filter(|l| l.key.level == LimitLevel::Never)
        .collect();
    assert_eq!(kept.len(), 1, "the entry is stored");
    assert_eq!(kept[0].by, LimitMaker::Person);
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
    let made = router.seams.grants.limits();
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].by, LimitMaker::Assistant);
    assert_eq!(made[0].said, None);
    assert_eq!(made[0].key.space, SpaceScope::Only(space("work")));
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
    let kept = router.seams.grants.limits();
    assert_eq!(
        kept[0].said.as_ref().map(|s| s.as_str()),
        Some("tidy my inbox")
    );
    assert_eq!(kept[0].key.level, LimitLevel::AlwaysAsk);
}
