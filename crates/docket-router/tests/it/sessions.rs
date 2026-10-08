//! Sessions, turns and task policies: derived from the person's words, narrowed silently,
//! widened only with their confirmation, never wider than a parent's.

use crate::support::*;
use docket_core::*;
use docket_fake::{ScriptedConfirmer, ScriptedReader, ScriptedWriter};
use prov::{AgentRef, ConfirmReceipt, Effect, InputProof, UnixSeconds};
use std::collections::BTreeSet;

fn yes() -> ConfirmAnswer {
    ConfirmAnswer::Allowed {
        scope: GrantScope::Once,
        receipt: ConfirmReceipt {
            id: prov::ConfirmId::parse("c-1").expect("id"),
            input: InputProof::HardwareSeat,
            at: UnixSeconds(1),
            covers: prov::Confidentiality::Secret,
        },
    }
}

fn narrow(task: &prov::TaskId) -> TaskPolicy {
    TaskPolicy {
        actions: BTreeSet::from([ActionMatch::AppUpTo(mail_app(), Effect::UndoableWrite)]),
        ceiling: Effect::UndoableWrite,
        ..wide_policy(task, "work")
    }
}

async fn policy_of(
    router: &docket_router::Router<docket_fake::FakeSeams>,
    s: &prov::SessionId,
) -> Option<TaskPolicy> {
    match ask(
        router,
        &companion(),
        IntentsRequest::SessionTaskPolicy { session: s.clone() },
    )
    .await
    {
        IntentsReply::TaskPolicy(p) => p.map(|b| *b),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_turn_derives_a_policy_stamped_by_the_router() {
    let mut router = router();
    let s = open(&router, "work", AgentRef::Companion).await;
    router.seams.writer = ScriptedWriter::returning(Ok(TaskPolicy {
        from: vec![],
        expires: UnixSeconds(i64::MAX),
        ..narrow(&s.task)
    }));
    let turn = say(&router, &s.session, "archive the newsletters").await;
    let policy = policy_of(&router, &s.session).await.expect("a policy");
    assert_eq!(
        policy.from,
        vec![turn],
        "the person's turns, as the router holds them"
    );
    assert_eq!(
        policy.expires,
        UnixSeconds(3600),
        "lapses by the configured maximum, whatever the writer said"
    );
    assert_eq!(policy.task, s.task);
    assert_eq!(router.seams.writer.calls(), vec![(s.task.clone(), 1)]);
}

#[tokio::test]
async fn writer_failure_means_every_non_read_call_is_outside() {
    let router = router();
    let s = open(&router, "work", AgentRef::Companion).await;
    say(&router, &s.session, "send this to accounting").await;
    grant_mail(&router, "work");
    assert_eq!(policy_of(&router, &s.session).await, None);
    ask(
        &router,
        &companion(),
        IntentsRequest::Suggest(SuggestAsk {
            action: action("mail.message.send"),
            param: param("to"),
            typed: String::new(),
        }),
    )
    .await;
    let send = call(
        "mail.message.send",
        &[],
        vec![
            ("to", Value::Entity(entity("mail.contact", "c1"))),
            ("body", Value::Text("send".into())),
        ],
    );
    let refused = perform(&router, send)
        .await
        .expect_err("outside the policy: asks");
    assert_eq!(refused, CallRefusal::Unconfirmed(ConfirmEnd::Dismissed));
    assert_eq!(router.seams.reviewer.call_count(), 0);
    assert!(router.seams.link.mail.sent().is_empty());
}

#[tokio::test]
async fn narrowing_is_silent_widening_confirms() {
    let mut router = router();
    let s = open(&router, "work", AgentRef::Companion).await;
    router.seams.writer = ScriptedWriter::returning(Ok(wide_policy(&s.task, "work")));
    say(&router, &s.session, "tidy my inbox").await;
    assert_eq!(
        policy_of(&router, &s.session)
            .await
            .expect("policy")
            .ceiling,
        Effect::Destructive
    );
    // A narrower policy replaces it without asking.
    router.seams.writer = ScriptedWriter::returning(Ok(narrow(&s.task)));
    say(&router, &s.session, "only archive").await;
    assert_eq!(
        policy_of(&router, &s.session)
            .await
            .expect("policy")
            .ceiling,
        Effect::UndoableWrite
    );
    assert!(
        router.seams.confirmer.requests().is_empty(),
        "narrowing never asks"
    );
    // A wider one asks, quoting the person; a refusal leaves the narrow one.
    router.seams.writer = ScriptedWriter::returning(Ok(wide_policy(&s.task, "work")));
    say(&router, &s.session, "actually do anything").await;
    let requests = router.seams.confirmer.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0]
            .lines
            .iter()
            .any(|l| matches!(&l.value, Shown::Plain(t) if t == "actually do anything"))
    );
    assert_eq!(
        policy_of(&router, &s.session)
            .await
            .expect("policy")
            .ceiling,
        Effect::UndoableWrite
    );
    // And a yes widens it.
    router.seams.confirmer = ScriptedConfirmer::answering(vec![yes()]);
    say(&router, &s.session, "yes, anything").await;
    assert_eq!(
        policy_of(&router, &s.session)
            .await
            .expect("policy")
            .ceiling,
        Effect::Destructive
    );
}

#[tokio::test]
async fn widen_asks_even_for_a_first_policy_and_needs_a_turn_the_router_holds() {
    let mut router = router();
    let s = open(&router, "work", AgentRef::Companion).await;
    let turn = say(&router, &s.session, "tidy").await;
    let widen = |turn: TurnId| IntentsRequest::SessionWiden {
        session: s.session.clone(),
        widen: WidenAsk {
            turn,
            change: narrow(&s.task),
        },
    };
    let forged = ask(&router, &launcher(), widen(TurnId(9999))).await;
    assert_eq!(
        forged,
        IntentsReply::Refused(WireRefusal::Malformed),
        "no such turn"
    );
    let refused = ask(&router, &launcher(), widen(turn)).await;
    assert_eq!(
        refused,
        IntentsReply::Widened(WidenAnswer::Refused(ConfirmEnd::Dismissed))
    );
    assert_eq!(policy_of(&router, &s.session).await, None);
    router.seams.confirmer = ScriptedConfirmer::answering(vec![yes()]);
    let applied = ask(&router, &launcher(), widen(turn)).await;
    assert_eq!(applied, IntentsReply::Widened(WidenAnswer::Applied));
    assert!(policy_of(&router, &s.session).await.is_some());
}

#[tokio::test]
async fn a_policy_is_never_derived_from_content() {
    let mut router = router();
    let s = open(&router, "work", AgentRef::Companion).await;
    router.seams.writer = ScriptedWriter::returning(Ok(narrow(&s.task)));
    say(&router, &s.session, "tidy my inbox").await;
    grant_mail(&router, "work");
    let before = policy_of(&router, &s.session).await;
    // Hostile text asks for more; the router reads it, the writer never does.
    perform(&router, call("mail.thread.read", &["t1"], vec![]))
        .await
        .expect("read");
    assert_eq!(policy_of(&router, &s.session).await, before);
    assert_eq!(
        router.seams.writer.calls().len(),
        1,
        "only the person's turn reached the writer"
    );
}

#[tokio::test]
async fn a_prompt_field_caps_the_policy_to_its_app_and_reads_elsewhere() {
    let mut router = router();
    let s = open(&router, "work", AgentRef::Companion).await;
    let both = TaskPolicy {
        actions: BTreeSet::from([
            ActionMatch::AppUpTo(mail_app(), Effect::Destructive),
            ActionMatch::AppUpTo(app("org.quire.Files"), Effect::Destructive),
        ]),
        ..wide_policy(&s.task, "work")
    };
    router.seams.writer = ScriptedWriter::returning(Ok(both));
    let field = caller("org.quire.Mail", CallerRole::Field);
    // The field's app opens the session, so its turn is recorded from it.
    let reply = ask(
        &router,
        &field,
        IntentsRequest::SessionOpen(SessionOpen {
            space: space("work"),
            agent: AgentRef::User,
            parent: None,
            cwd: None,
            started_from: None,
            external: None,
        }),
    )
    .await;
    let IntentsReply::SessionOpened(opened) = reply else {
        panic!("{reply:?}")
    };
    let _ = s;
    let turn = IntentsRequest::SessionTurn {
        session: opened.session.clone(),
        turn: TurnIn {
            text: "archive these".into(),
            origin: Origin::InWindowField,
            keep: ContextKeep {
                query: Keep::Dropped,
                results: Keep::Dropped,
                selection: Keep::Kept,
                window: Keep::Dropped,
            },
            via: TurnVia::Typed,
        },
    };
    assert!(matches!(
        ask(&router, &field, turn).await,
        IntentsReply::TurnRecorded(_)
    ));
    let policy = policy_of(&router, &opened.session).await.expect("policy");
    let files = ActionMatch::AppUpTo(app("org.quire.Files"), Effect::Read);
    assert!(
        policy
            .actions
            .contains(&ActionMatch::AppUpTo(mail_app(), Effect::Destructive))
    );
    assert!(policy.actions.contains(&files), "{:?}", policy.actions);
}

#[tokio::test]
async fn a_child_task_is_never_wider_than_its_parent() {
    let router = router();
    let parent = open(&router, "work", AgentRef::Companion).await;
    give_policy(&router, &parent.session, narrow(&parent.task));
    let reply = ask(
        &router,
        &companion(),
        IntentsRequest::SessionOpen(SessionOpen {
            space: space("work"),
            agent: AgentRef::Worker {
                task: prov::TaskId::parse("t-w1").expect("task"),
            },
            parent: Some(parent.task.clone()),
            cwd: None,
            started_from: None,
            external: None,
        }),
    )
    .await;
    let IntentsReply::SessionOpened(child) = reply else {
        panic!("{reply:?}")
    };
    let got = policy_of(&router, &child.session).await.expect("inherited");
    assert!(matches!(
        compare(&got, &narrow(&parent.task)),
        PolicyChange::Narrows | PolicyChange::Same
    ));
    assert_eq!(got.task, child.task);
    let st = router.state.lock().expect("lock");
    assert_eq!(
        st.tasks.get(&child.task).expect("task").parent,
        Some(parent.task.clone())
    );
}

#[tokio::test]
async fn closing_a_task_leaves_its_skeleton_as_an_episode() {
    let router = router();
    let s = ready(&router).await;
    perform(&router, call("mail.thread.archive", &["t2"], vec![]))
        .await
        .expect("ran");
    let reply = ask(
        &router,
        &companion(),
        IntentsRequest::SessionClose {
            session: s.session.clone(),
        },
    )
    .await;
    assert_eq!(reply, IntentsReply::Done);
    let episodes: Vec<_> = router
        .seams
        .sink
        .records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Episode(e) => Some(e),
            _ => None,
        })
        .collect();
    assert_eq!(episodes.len(), 1);
    assert_eq!(episodes[0].skeleton.steps.len(), 1);
    assert_eq!(episodes[0].skeleton.asked.len(), 1);
    let closed = perform(&router, call("mail.thread.archive", &["t1"], vec![])).await;
    assert!(closed.is_err(), "a closed session takes no more calls");
}

#[tokio::test]
async fn only_the_reader_may_resolve_a_handle_and_the_screen_may_display_it() {
    let router = router();
    let s = ready(&router).await;
    let h = hold(&router, &s.session, "secret words", mail_label("work"));
    let resolve = IntentsRequest::SessionResolve {
        session: s.session.clone(),
        handle: h,
    };
    assert_eq!(
        ask(&router, &companion(), resolve.clone()).await,
        IntentsReply::Refused(WireRefusal::NotAllowed),
        "a planner cannot read what it holds"
    );
    let reader = caller("org.quire.Readerd", CallerRole::Reader);
    assert_eq!(
        ask(&router, &reader, resolve).await,
        IntentsReply::Resolved(Resolved {
            text: "secret words".into(),
            label: mail_label("work"),
        }),
        "the reader gets the text with the handle's own label"
    );
    let display = IntentsRequest::SessionDisplay {
        session: s.session.clone(),
        handle: h,
    };
    assert_eq!(
        ask(&router, &companion(), display.clone()).await,
        IntentsReply::Refused(WireRefusal::NotAllowed)
    );
    assert_eq!(
        ask(&router, &launcher(), display).await,
        IntentsReply::Text("secret words".into())
    );
    let st = router.state.lock().expect("lock");
    assert_eq!(
        st.sessions[&s.session].saw.untrusted,
        Saw::Seen,
        "the reader has now seen it"
    );
}

#[tokio::test]
async fn the_reader_answers_a_closed_set_plainly_and_any_text_as_a_handle() {
    let mut router = router();
    let s = ready(&router).await;
    let h = hold(
        &router,
        &s.session,
        "thanks, see attached",
        mail_label("work"),
    );
    let choice = ChoiceId::parse("receipt").expect("id");
    let ask_for = |want: ValueSchema| IntentsRequest::SessionRead {
        session: s.session.clone(),
        ask: ReadAsk {
            ask: ReaderAsk {
                inputs: vec![h],
                want,
                task: ReaderTask::Classify,
            },
        },
    };
    router.seams.reader = ScriptedReader::answering(vec![
        Ok(Value::Choice(choice.clone())),
        Ok(Value::Text("a receipt".into())),
        Ok(Value::Integer(99)),
    ]);
    let plain = ask(
        &router,
        &companion(),
        ask_for(ValueSchema::Choice(vec![choice.clone()])),
    )
    .await;
    assert_eq!(
        plain,
        IntentsReply::Read(Reveal::Plain(Value::Choice(choice)))
    );
    let words = ask(
        &router,
        &companion(),
        ask_for(ValueSchema::Text { max: CharCount(50) }),
    )
    .await;
    let IntentsReply::Read(Reveal::Handle(held)) = words else {
        panic!("{words:?}")
    };
    {
        let st = router.state.lock().expect("lock");
        let record = &st.sessions[&s.session];
        assert_eq!(record.handles.display(held), Some("a receipt"));
        assert_eq!(
            record.handles.label(held),
            Some(&mail_label("work")),
            "the join of the inputs"
        );
    }
    let wrong = ask(
        &router,
        &companion(),
        ask_for(ValueSchema::Integer { min: 0, max: 9 }),
    )
    .await;
    assert_eq!(
        wrong,
        IntentsReply::Refused(WireRefusal::Read(ReadFault::OutOfSchema(
            SchemaFault::OutOfRange
        ))),
        "an answer outside the schema"
    );
    assert_eq!(
        router.seams.reader.sessions(),
        vec![s.session.clone(); 3],
        "the router names the session the handles are held in on every read"
    );
}

#[tokio::test]
async fn a_read_that_gives_no_answer_says_why() {
    let mut router = router();
    let s = ready(&router).await;
    let h = hold(&router, &s.session, "thanks", mail_label("work"));
    let read = |input: Handle| IntentsRequest::SessionRead {
        session: s.session.clone(),
        ask: ReadAsk {
            ask: ReaderAsk {
                inputs: vec![input],
                want: ValueSchema::Date,
                task: ReaderTask::Classify,
            },
        },
    };
    router.seams.reader = ScriptedReader::answering(vec![
        Err(ReaderError::Unparseable),
        Err(ReaderError::OutOfSchema(SchemaFault::NotRepresentable)),
        Err(ReaderError::ModelUnavailable),
    ]);
    let rows = [
        (Handle(999), ReadFault::NotHeld),
        (h, ReadFault::Unparseable),
        (h, ReadFault::OutOfSchema(SchemaFault::NotRepresentable)),
        (h, ReadFault::Unavailable),
    ];
    for (input, fault) in rows {
        assert_eq!(
            ask(&router, &companion(), read(input)).await,
            IntentsReply::Refused(WireRefusal::Read(fault.clone())),
            "{fault:?}"
        );
    }
}

#[tokio::test]
async fn a_thing_given_to_a_read_is_held_but_not_text_and_a_stranger_is_not_held() {
    let mut router = router();
    let s = ready(&router).await;
    let thread = {
        let mut st = router.state.lock().expect("lock");
        let record = st.sessions.get_mut(&s.session).expect("session");
        record
            .handles
            .mint_entity(entity("mail.thread", "t1"), mail_label("work"))
    };
    let text = hold(&router, &s.session, "thanks", mail_label("work"));
    let read = |inputs: Vec<Handle>| IntentsRequest::SessionRead {
        session: s.session.clone(),
        ask: ReadAsk {
            ask: ReaderAsk {
                inputs,
                want: ValueSchema::Date,
                task: ReaderTask::Classify,
            },
        },
    };
    router.seams.reader = ScriptedReader::answering(vec![]);
    let kind = prov::EntityKind::parse("mail.thread").expect("kind");
    let rows = [
        (
            vec![thread],
            ReadFault::NotText {
                handle: thread,
                shape: HandleShape::Entity(kind.clone()),
            },
        ),
        (
            vec![text, thread],
            ReadFault::NotText {
                handle: thread,
                shape: HandleShape::Entity(kind),
            },
        ),
        (vec![Handle(999), thread], ReadFault::NotHeld),
    ];
    for (inputs, fault) in rows {
        assert_eq!(
            ask(&router, &companion(), read(inputs)).await,
            IntentsReply::Refused(WireRefusal::Read(fault.clone())),
            "{fault:?}"
        );
    }
}

#[tokio::test]
async fn closing_and_turns_belong_to_the_session_the_surface_opened() {
    let router = router();
    let field = caller("org.quire.Mail", CallerRole::Field);
    let other = caller("org.quire.Files", CallerRole::Field);
    let IntentsReply::SessionOpened(opened) = ask(
        &router,
        &field,
        IntentsRequest::SessionOpen(SessionOpen {
            space: space("work"),
            agent: AgentRef::User,
            parent: None,
            cwd: None,
            started_from: None,
            external: None,
        }),
    )
    .await
    else {
        panic!("open")
    };
    let close = IntentsRequest::SessionClose {
        session: opened.session.clone(),
    };
    assert_eq!(
        ask(&router, &other, close.clone()).await,
        IntentsReply::Refused(WireRefusal::NotAllowed)
    );
    assert_eq!(ask(&router, &field, close).await, IntentsReply::Done);
    let nowhere = IntentsRequest::SessionClose {
        session: prov::SessionId::parse("s-999").expect("id"),
    };
    assert_eq!(
        ask(&router, &field, nowhere).await,
        IntentsReply::Refused(WireRefusal::NoSuchSession)
    );
}
