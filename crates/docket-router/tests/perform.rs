//! A call through the router over the fakes: what runs, what asks and what is refused.

mod support;

use action_review::{ReviewReason, ReviewVerdict};
use docket_core::*;
use docket_fake::{ReviewMode, ScriptedReviewer};
use docket_router::{GrantStore, SessionState};
use prov::{AgentRef, Integrity, Labelled};
use support::*;

fn contact() -> Value {
    Value::Entity(entity("mail.contact", "c1"))
}

fn stranger() -> Value {
    Value::Entity(entity("mail.contact", "eve@evil.test"))
}

fn send(to: Value) -> CallRequest {
    call(
        "mail.message.send",
        &[],
        vec![("to", to), ("body", Value::Text("tidy".into()))],
    )
}

async fn show_contacts(router: &docket_router::Router<docket_fake::FakeSeams>) {
    let reply = ask(
        router,
        &companion(),
        IntentsRequest::Suggest(SuggestAsk {
            action: action("mail.message.send"),
            param: param("to"),
            typed: String::new(),
        }),
    )
    .await;
    assert!(
        matches!(reply, IntentsReply::Suggestions(ref s) if s.len() == 1),
        "{reply:?}"
    );
}

fn calls(router: &docket_router::Router<docket_fake::FakeSeams>) -> Vec<AuditRecord> {
    router
        .seams
        .sink
        .records()
        .into_iter()
        .filter(|r| matches!(r, AuditRecord::Call { .. }))
        .collect()
}

fn deny() -> Result<ReviewVerdict, ReviewError> {
    Ok(ReviewVerdict::Deny {
        why: ReviewReason {
            code: ReasonCode::OutsideRequest,
            text: ReasonText("no".into()),
        },
    })
}

#[tokio::test]
async fn read_runs_without_confirm() {
    let router = router();
    ready(&router).await;
    let outcome = perform(&router, call("mail.thread.read", &["t1"], vec![]))
        .await
        .expect("ran");
    assert!(outcome.value.is_some());
    assert!(router.seams.confirmer.requests().is_empty());
    assert_eq!(router.seams.reviewer.call_count(), 0);
}

#[tokio::test]
async fn a_planner_gets_untrusted_text_as_a_handle_never_plain() {
    let router = router();
    let s = ready(&router).await;
    let outcome = perform(&router, call("mail.thread.read", &["t1"], vec![]))
        .await
        .expect("ran");
    let value = outcome.value.expect("value");
    let Value::Handle(h) = value.value else {
        panic!("the thread body reached the planner as {value:?}")
    };
    assert_eq!(value.label.integrity, Integrity::Untrusted);
    assert_eq!(
        outcome.show,
        Preview::None,
        "a preview may quote the content"
    );
    let st = router.state.lock().expect("lock");
    let record = &st.sessions[&s.session];
    assert!(
        record
            .handles
            .display(h)
            .expect("held")
            .contains("Ignore previous")
    );
    assert_eq!(record.saw.untrusted, Saw::Seen);
    assert_eq!(record.saw.private, Saw::Seen, "a thread of mail is private");
    assert_eq!(
        record.state,
        SessionState::Open(docket_router::Taint::Tainted)
    );
}

#[tokio::test]
async fn under_default_an_outbound_act_always_asks_whatever_is_trusted() {
    let router = router();
    ready(&router).await;
    show_contacts(&router).await;
    let refused = perform(&router, send(contact()))
        .await
        .expect_err("asked, then dismissed");
    assert_eq!(refused, CallRefusal::Unconfirmed(ConfirmEnd::Dismissed));
    assert_eq!(router.seams.confirmer.requests().len(), 1);
    assert_eq!(
        router.seams.reviewer.call_count(),
        0,
        "QUESTIONS S1: no reviewer stands in for the person under Default"
    );
    assert!(router.seams.link.mail.sent().is_empty());
}

#[tokio::test]
async fn under_trust_more_an_outbound_act_with_every_sink_trusted_is_reviewed_by_every_stage_then_runs()
 {
    let mut router = router();
    router.config.strictness = Strictness::TrustMore;
    ready(&router).await;
    show_contacts(&router).await;
    perform(&router, send(contact())).await.expect("ran");
    assert_eq!(
        router.seams.reviewer.call_count(),
        3,
        "quick, deliberate, second opinion"
    );
    assert_eq!(router.seams.link.mail.sent().len(), 1);
    let reviews = router
        .seams
        .sink
        .records()
        .iter()
        .filter(|r| matches!(r, AuditRecord::Review { .. }))
        .count();
    assert_eq!(reviews, 3, "every verdict is logged");
}

#[tokio::test]
async fn a_reviewer_that_asks_sends_the_call_to_the_person() {
    let mut router = router();
    router.config.strictness = Strictness::TrustMore;
    router.seams.reviewer = ScriptedReviewer::always_ask();
    ready(&router).await;
    show_contacts(&router).await;
    let refused = perform(&router, send(contact()))
        .await
        .expect_err("asked, then dismissed");
    assert_eq!(refused, CallRefusal::Unconfirmed(ConfirmEnd::Dismissed));
    assert_eq!(router.seams.confirmer.requests().len(), 1);
    assert!(router.seams.link.mail.sent().is_empty());
}

#[tokio::test]
async fn a_reviewer_that_hangs_past_its_deadline_is_a_timeout_that_asks() {
    let mut router = router();
    router.config.strictness = Strictness::TrustMore;
    router.config.review = docket_core::ReviewTimeouts {
        quick: docket_core::Millis(0),
        deliberate: docket_core::Millis(0),
        second: docket_core::Millis(0),
    };
    router.seams.reviewer = ScriptedReviewer::hanging();
    ready(&router).await;
    show_contacts(&router).await;
    let refused = perform(&router, send(contact()))
        .await
        .expect_err("timed out, asked, then dismissed");
    assert_eq!(refused, CallRefusal::Unconfirmed(ConfirmEnd::Dismissed));
    assert_eq!(router.seams.confirmer.requests().len(), 1);
    assert!(router.seams.link.mail.sent().is_empty());
    let timed_out = router.seams.sink.records().iter().any(|r| {
        matches!(r, AuditRecord::Review { mark, .. }
            if mark.stage == Stage::Quick && mark.verdict == Err(ReviewError::Timeout))
    });
    assert!(timed_out, "the stage's timeout is logged");
}

#[tokio::test]
async fn an_untrusted_recipient_asks_whatever_the_judge_says() {
    let router = router();
    ready(&router).await;
    let refused = perform(&router, send(stranger()))
        .await
        .expect_err("asked, then dismissed");
    assert_eq!(refused, CallRefusal::Unconfirmed(ConfirmEnd::Dismissed));
    assert_eq!(
        router.seams.reviewer.call_count(),
        0,
        "a hijacked judge is never consulted"
    );
    assert!(router.seams.link.mail.sent().is_empty());
    let requests = router.seams.confirmer.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0]
            .why
            .contains(&AskReason::UntrustedSink(ArgSink::Recipient)),
        "{:?}",
        requests[0].why
    );
    assert!(
        requests[0]
            .lines
            .iter()
            .any(|l| matches!(&l.value, Shown::Quoted { text, .. } if text == "eve@evil.test")),
        "somebody else's words are drawn quoted"
    );
}

#[tokio::test]
async fn planner_taint_is_computed_by_the_router_not_the_caller() {
    let router = router();
    ready(&router).await;
    let mut lying = send(stranger());
    for arg in lying.args.values_mut() {
        arg.label = trusted();
    }
    let refused = perform(&router, lying).await.expect_err("asked");
    assert!(
        matches!(refused, CallRefusal::Unconfirmed(_)),
        "{refused:?}"
    );
    assert_eq!(router.seams.link.mail.sent().len(), 0);
}

#[tokio::test]
async fn a_handle_brings_back_the_label_it_was_minted_with() {
    let router = router();
    let s = ready(&router).await;
    let h = hold(&router, &s.session, "eve@evil.test", mail_label("work"));
    let call = CallRequest {
        action: action("mail.message.send"),
        target: TargetValue::Nothing,
        args: [
            (
                param("to"),
                Labelled {
                    value: Value::Handle(h),
                    label: trusted(),
                },
            ),
            (
                param("body"),
                Labelled {
                    value: Value::Text("tidy".into()),
                    label: trusted(),
                },
            ),
        ]
        .into(),
        origin: Origin::Companion,
    };
    // The handle holds text and `to` wants an entity: the call is refused before the gate.
    let refused = perform(&router, call).await.expect_err("wrong type");
    assert!(
        matches!(
            refused,
            CallRefusal::BadArgs {
                why: ArgFault::WrongType,
                ..
            }
        ),
        "{refused:?}"
    );
    let body = CallRequest {
        action: action("mail.draft.create"),
        target: TargetValue::Nothing,
        args: [(
            param("body"),
            Labelled {
                value: Value::Handle(h),
                label: trusted(),
            },
        )]
        .into(),
        origin: Origin::Companion,
    };
    perform(&router, body)
        .await
        .expect("a draft of held text is allowed, and reviewed");
    let records = router.seams.sink.records();
    assert!(
        records
            .iter()
            .any(|r| matches!(r, AuditRecord::Review { .. })),
        "an untrusted body makes the judge look"
    );
    let unknown = CallRequest {
        args: [(
            param("body"),
            Labelled {
                value: Value::Handle(Handle(999)),
                label: trusted(),
            },
        )]
        .into(),
        ..call_draft()
    };
    let refused = perform(&router, unknown).await.expect_err("no such handle");
    assert_eq!(
        refused,
        CallRefusal::BadArgs {
            param: param("body"),
            why: ArgFault::UnknownHandle
        }
    );
}

fn call_draft() -> CallRequest {
    call(
        "mail.draft.create",
        &[],
        vec![("body", Value::Text("x".into()))],
    )
}

#[tokio::test]
async fn bad_arguments_end_the_call_with_the_parameter_named() {
    let router = router();
    ready(&router).await;
    let cases: Vec<(&str, CallRequest, CallRefusal)> = vec![
        (
            "a required parameter is missing",
            call("mail.draft.create", &[], vec![]),
            CallRefusal::BadArgs {
                param: param("body"),
                why: ArgFault::Missing,
            },
        ),
        (
            "a parameter of the wrong type",
            call("mail.draft.create", &[], vec![("body", Value::Integer(3))]),
            CallRefusal::BadArgs {
                param: param("body"),
                why: ArgFault::WrongType,
            },
        ),
        (
            "a parameter nobody declared",
            call(
                "mail.draft.create",
                &[],
                vec![
                    ("body", Value::Text("x".into())),
                    ("cc", Value::Text("y".into())),
                ],
            ),
            CallRefusal::BadArgs {
                param: param("cc"),
                why: ArgFault::WrongType,
            },
        ),
        (
            "no target for an action on threads",
            call("mail.thread.archive", &[], vec![]),
            CallRefusal::BadArgs {
                param: param("target"),
                why: ArgFault::Missing,
            },
        ),
        (
            "an action nobody declared",
            call("mail.thread.explode", &["t1"], vec![]),
            CallRefusal::NoSuchAction(action("mail.thread.explode")),
        ),
    ];
    for (name, request, want) in cases {
        let got = perform(&router, request).await.expect_err(name);
        assert_eq!(got, want, "case: {name}");
    }
    assert_eq!(
        calls(&router).len(),
        5,
        "every call, refused or not, leaves one record"
    );
}

#[tokio::test]
async fn a_tainted_session_voids_an_always_grant() {
    let router = router();
    ready(&router).await;
    perform(&router, call("mail.thread.archive", &["t2"], vec![]))
        .await
        .expect("clean: runs");
    perform(&router, call("mail.thread.read", &["t1"], vec![]))
        .await
        .expect("read");
    let refused = perform(&router, call("mail.thread.archive", &["t1"], vec![]))
        .await
        .expect_err("tainted: asks");
    assert_eq!(refused, CallRefusal::Unconfirmed(ConfirmEnd::Dismissed));
    let requests = router.seams.confirmer.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].why.contains(&AskReason::FirstUse),
        "{:?}",
        requests[0].why
    );
    assert_eq!(requests[0].offer, ConfirmOffer::OnceOnly);
    assert!(matches!(requests[0].taint, TaintNote::ReadUntrusted(_)));
}

#[tokio::test]
async fn a_task_policy_cannot_exceed_the_persons_grants() {
    let router = router();
    let s = open(&router, "work", AgentRef::Companion).await;
    say(&router, &s.session, "tidy my inbox").await;
    give_policy(&router, &s.session, wide_policy(&s.task, "work"));
    // Policy covers Mail; no grant says the companion may use it.
    let refused = perform(&router, call("mail.thread.archive", &["t1"], vec![]))
        .await
        .expect_err("asks");
    assert_eq!(refused, CallRefusal::Unconfirmed(ConfirmEnd::Dismissed));
    assert!(
        router.seams.confirmer.requests()[0]
            .why
            .contains(&AskReason::FirstUse)
    );
}

#[tokio::test]
async fn an_always_from_the_person_is_remembered() {
    let mut router = router();
    let receipt = prov::ConfirmReceipt {
        id: prov::ConfirmId::parse("c-9").expect("id"),
        input: prov::InputProof::HardwareSeat,
        at: prov::UnixSeconds(1),
        covers: prov::Confidentiality::Secret,
    };
    router.seams.confirmer =
        docket_fake::ScriptedConfirmer::answering(vec![ConfirmAnswer::Allowed {
            scope: GrantScope::Always,
            receipt,
        }]);
    let s = open(&router, "work", AgentRef::Companion).await;
    say(&router, &s.session, "tidy my inbox").await;
    give_policy(&router, &s.session, wide_policy(&s.task, "work"));
    perform(&router, call("mail.thread.archive", &["t1"], vec![]))
        .await
        .expect("asked, allowed");
    perform(&router, call("mail.thread.archive", &["t2"], vec![]))
        .await
        .expect("not asked again");
    assert_eq!(router.seams.confirmer.requests().len(), 1);
    assert_eq!(
        router.seams.grants.grants().len(),
        1,
        "one grant per data class the action touches (archive touches mail)"
    );
}

#[tokio::test]
async fn an_action_hidden_from_agents_is_denied_with_only_a_coarse_code() {
    let router = router();
    ready(&router).await;
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
        args: Args::new(),
        origin: Origin::Companion,
    };
    let refused = perform(&router, forget).await.expect_err("denied");
    assert_eq!(refused, CallRefusal::Denied(DenyCode::NotAllowed));
}

#[tokio::test]
async fn an_exact_repeat_after_a_denial_is_refused_without_review() {
    let mut router = router();
    router.seams.reviewer = ScriptedReviewer::queued(vec![deny()], ReviewMode::AlwaysAllow);
    router.config.strictness = Strictness::AskMore;
    ready(&router).await;
    let first = perform(&router, call("mail.thread.archive", &["t1"], vec![]))
        .await
        .expect_err("denied");
    assert_eq!(first, CallRefusal::Denied(DenyCode::NotAllowed));
    assert_eq!(router.seams.reviewer.call_count(), 1);
    let again = perform(&router, call("mail.thread.archive", &["t1"], vec![]))
        .await
        .expect_err("repeat");
    assert_eq!(again, CallRefusal::Denied(DenyCode::Repeated));
    assert_eq!(
        router.seams.reviewer.call_count(),
        1,
        "the repeat never reached the reviewer"
    );
    assert!(!router.seams.link.mail.is_archived("t1"));
}

#[tokio::test]
async fn three_denials_pause_the_session_until_the_person_speaks() {
    let router = router();
    let s = ready(&router).await;
    for who in ["a@evil.test", "b@evil.test", "c@evil.test"] {
        let refused = perform(&router, send(Value::Entity(entity("mail.contact", who))))
            .await
            .expect_err("asked, dismissed");
        assert!(matches!(refused, CallRefusal::Unconfirmed(_)));
    }
    let paused = perform(&router, call("mail.thread.read", &["t2"], vec![]))
        .await
        .expect_err("paused");
    assert_eq!(paused, CallRefusal::Paused(BreakerTrip::Probing));
    let records = router.seams.sink.records();
    assert!(records.iter().any(|r| matches!(
        r,
        AuditRecord::Breaker {
            trip: BreakerTrip::Probing,
            ..
        }
    )));
    say(&router, &s.session, "carry on").await;
    perform(&router, call("mail.thread.read", &["t2"], vec![]))
        .await
        .expect("the person spoke");
}

#[tokio::test]
async fn halt_overrides_a_reviewer_allow() {
    let router = router();
    ready(&router).await;
    let reply = ask(
        &router,
        &control(),
        IntentsRequest::ControlHalt {
            scope: prov::SpaceScope::Only(space("work")),
            cause: HaltCause::StopKey,
        },
    )
    .await;
    assert_eq!(reply, IntentsReply::Done);
    let refused = perform(&router, call("mail.thread.read", &["t1"], vec![]))
        .await
        .expect_err("halted");
    assert_eq!(
        refused,
        CallRefusal::Halted(prov::SpaceScope::Only(space("work")))
    );
    assert_eq!(router.seams.reviewer.call_count(), 0);
}

#[tokio::test]
async fn an_exhausted_budget_overrides_an_allow() {
    let router = router();
    let s = ready(&router).await;
    {
        let mut st = router.state.lock().expect("lock");
        let record = st.sessions.get_mut(&s.session).expect("session");
        record.ledger.calls = router.config.budget.calls;
    }
    let refused = perform(&router, call("mail.thread.read", &["t1"], vec![]))
        .await
        .expect_err("budget");
    assert_eq!(refused, CallRefusal::OverBudget(BudgetKind::Calls));
}

#[tokio::test]
async fn a_fan_out_over_the_mass_threshold_asks() {
    let router = router();
    ready(&router).await;
    let keys: Vec<String> = (0..25).map(|n| format!("t{n}")).collect();
    let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    let refused = perform(&router, call("mail.thread.archive", &refs, vec![]))
        .await
        .expect_err("asks");
    assert_eq!(refused, CallRefusal::Unconfirmed(ConfirmEnd::Dismissed));
    let why = &router.seams.confirmer.requests()[0].why;
    assert!(
        why.iter()
            .any(|r| matches!(r, AskReason::Mass(n) if n.0 == 25)),
        "{why:?}"
    );
}

#[tokio::test]
async fn every_call_appends_exactly_one_audit_record() {
    let router = router();
    ready(&router).await;
    let before = calls(&router).len();
    perform(&router, call("mail.thread.read", &["t1"], vec![]))
        .await
        .expect("ran");
    let _ = perform(&router, send(stranger())).await;
    let _ = perform(&router, call("mail.draft.create", &[], vec![])).await;
    assert_eq!(calls(&router).len(), before + 3);
}

#[tokio::test]
async fn an_undoable_act_is_journalled_under_its_actor_and_can_be_undone() {
    let router = router();
    let s = ready(&router).await;
    let outcome = perform(&router, call("mail.thread.archive", &["t1"], vec![]))
        .await
        .expect("ran");
    let Undoable::Journaled(row) = outcome.undo else {
        panic!("the caller is given the journal's row: {:?}", outcome.undo)
    };
    assert!(router.seams.link.mail.is_archived("t1"));
    let IntentsReply::Journal(rows) = ask(
        &router,
        &launcher(),
        IntentsRequest::ControlJournal(JournalFilter {
            run: None,
            session: Some(s.session.clone()),
            limit: porter_core::Count(10),
        }),
    )
    .await
    else {
        panic!("journal")
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, row, "the outcome names the journal's row");
    assert!(
        matches!(rows[0].actor, prov::Actor::Companion { .. }),
        "{:?}",
        rows[0].actor
    );
    assert_eq!(rows[0].state, UndoState::Available);
    let undone = ask(&router, &launcher(), IntentsRequest::Undo(rows[0].id)).await;
    assert_eq!(undone, IntentsReply::Undone(Ok(())));
    assert!(!router.seams.link.mail.is_archived("t1"));
    let IntentsReply::Journal(rows) = ask(
        &router,
        &launcher(),
        IntentsRequest::ControlJournal(JournalFilter {
            run: None,
            session: None,
            limit: porter_core::Count(10),
        }),
    )
    .await
    else {
        panic!("journal")
    };
    assert!(
        matches!(
            rows[0].state,
            UndoState::Undone {
                by: prov::Actor::User { .. }
            }
        ),
        "{:?}",
        rows[0].state
    );
}

#[tokio::test]
async fn undo_all_stops_at_the_first_conflict() {
    let router = router();
    let s = ready(&router).await;
    for key in ["t1", "t2"] {
        perform(&router, call("mail.thread.archive", &[key], vec![]))
            .await
            .expect("ran");
    }
    // Somebody changed t2 since: its token is gone, and it is the newest, so it is tried first.
    let newest = {
        let st = router.state.lock().expect("lock");
        st.journal.entries().last().expect("entry").clone()
    };
    use docket_client::IntentProvider;
    router
        .seams
        .link
        .mail
        .undo(newest.token.clone(), prov::Actor::Unknown)
        .await
        .expect("the person undid it by hand");
    let reply = ask(
        &router,
        &launcher(),
        IntentsRequest::UndoAll(UndoScope::Task(s.task.clone())),
    )
    .await;
    let IntentsReply::UndoneAll(report) = reply else {
        panic!("{reply:?}")
    };
    assert_eq!(report.undone.0, 0);
    assert_eq!(report.stopped, Some(UndoFault::Gone));
    assert!(
        router.seams.link.mail.is_archived("t1"),
        "the older entry was left alone"
    );
}

#[tokio::test]
async fn an_app_undo_marks_the_journal() {
    let router = router();
    ready(&router).await;
    perform(&router, call("mail.thread.archive", &["t1"], vec![]))
        .await
        .expect("ran");
    let id = router.state.lock().expect("lock").journal.entries()[0].id;
    let by = prov::Actor::User {
        via: app("org.quire.Mail"),
    };
    router.note_app_undo(id, by.clone());
    let st = router.state.lock().expect("lock");
    assert_eq!(
        st.journal.get(id).expect("row").state,
        UndoState::Undone { by }
    );
}

#[tokio::test]
async fn the_persons_own_launcher_act_skips_policy_and_leaves_no_session_behind() {
    let router = router();
    let reply = ask(
        &router,
        &launcher(),
        IntentsRequest::Perform {
            call: call("mail.thread.archive", &["t2"], vec![]),
            session: None,
            parent_window: None,
        },
    )
    .await;
    assert!(
        matches!(reply, IntentsReply::Performed(ref r) if r.is_ok()),
        "{reply:?}"
    );
    assert!(router.seams.link.mail.is_archived("t2"));
    assert!(router.seams.confirmer.requests().is_empty());
    assert!(router.state.lock().expect("lock").sessions.is_empty());
}

#[tokio::test]
async fn a_call_names_the_session_it_belongs_to_while_two_tasks_work() {
    let router = router();
    let first = ready(&router).await;
    let _newer = open(&router, "work", AgentRef::Companion).await;
    let reply = ask(
        &router,
        &companion(),
        IntentsRequest::Perform {
            call: call("mail.thread.archive", &["t1"], vec![]),
            session: Some(first.session.clone()),
            parent_window: None,
        },
    )
    .await;
    assert!(
        matches!(reply, IntentsReply::Performed(ref r) if r.is_ok()),
        "{reply:?}"
    );
    let acted_in = router
        .seams
        .sink
        .records()
        .into_iter()
        .find_map(|r| match r {
            AuditRecord::Call {
                actor: prov::Actor::Companion { session, .. },
                ..
            } => Some(session),
            _ => None,
        });
    assert_eq!(acted_in, Some(first.session), "not the newest session");
    let stranger = ask(
        &router,
        &companion(),
        IntentsRequest::Perform {
            call: call("mail.thread.archive", &["t2"], vec![]),
            session: Some(prov::SessionId::parse("s-999").expect("id")),
            parent_window: None,
        },
    )
    .await;
    assert_eq!(stranger, IntentsReply::Refused(WireRefusal::NoSuchSession));
}

#[tokio::test]
async fn an_app_that_never_answers_ends_the_call_as_a_timeout() {
    let router = router();
    ready(&router).await;
    router
        .seams
        .link
        .answer_from(&mail_app(), docket_fake::Answering::Silent);
    let refused = perform(&router, call("mail.thread.archive", &["t1"], vec![]))
        .await
        .expect_err("silent app");
    assert_eq!(refused, CallRefusal::Timeout);
    assert!(!router.seams.link.mail.is_archived("t1"));
}

#[tokio::test]
async fn an_app_that_is_not_there_ends_the_call_as_unavailable_not_as_a_failure() {
    let router = router();
    ready(&router).await;
    router
        .seams
        .link
        .answer_from(&mail_app(), docket_fake::Answering::Absent);
    let refused = perform(&router, call("mail.thread.archive", &["t1"], vec![]))
        .await
        .expect_err("absent app");
    assert_eq!(refused, CallRefusal::AppUnavailable(mail_app()));
}
