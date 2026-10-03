//! The fixtures validate and the fakes behave.

use action_review::{ReviewVerdict, Reviewer};
use docket_client::IntentProvider;
use docket_core::*;
use docket_fake::*;
use docket_router::{Clock, EventSink, GrantStore, Registry, parse};
use prov::{ActionName, Actor, AppName, SpaceId, UnixSeconds};
use std::collections::BTreeSet;

fn space() -> SpaceId {
    SpaceId::parse("work").expect("space")
}

fn mail() -> FakeMail {
    FakeMail::new(mail_manifest().expect("fixture"), space())
        .with_thread(MailThread {
            key: "t1".into(),
            subject: "Receipts".into(),
            from: "shop@example.test".into(),
            body: "hello".into(),
        })
        .with_contact(MailContact {
            key: "c1".into(),
            name: "Accounting".into(),
            address: "accounting@example.test".into(),
        })
}

fn invocation(action: &str, keys: &[&str]) -> Invocation {
    let app = AppName::parse("org.quire.Mail").expect("app");
    Invocation {
        call: CallId(1),
        action: ActionName::parse(action).expect("action"),
        target: TargetValue::Entities(
            keys.iter()
                .map(|k| prov::EntityId {
                    app: app.clone(),
                    kind: prov::EntityKind::parse("mail.thread").expect("kind"),
                    key: prov::EntityKey::parse(k).expect("key"),
                })
                .collect(),
        ),
        args: Args::new(),
        actor: Actor::User {
            via: AppName::parse("org.quire.Shell").expect("app"),
        },
        origin: Origin::Launcher,
        space: space(),
    }
}

#[test]
fn the_fixture_manifests_parse_and_validate() {
    let mail = mail_manifest().expect("mail");
    let files = files_manifest().expect("files");
    let effects = |m: &ValidManifest| -> Vec<(String, prov::Effect)> {
        m.manifest()
            .actions
            .iter()
            .map(|a| (a.name.as_str().to_owned(), a.effect))
            .collect()
    };
    assert!(effects(&mail).contains(&("mail.message.send".into(), prov::Effect::Outbound)));
    assert!(effects(&mail).contains(&("mail.thread.delete".into(), prov::Effect::Destructive)));
    assert!(effects(&files).contains(&("files.file.move".into(), prov::Effect::UndoableWrite)));
    let titles = &mail.manifest().entities[0].titles;
    assert_eq!(*titles, TitleTrust::ThirdParty(prov::Source::Mail));
}

#[test]
fn the_fixtures_resolve_every_kind_when_both_are_loaded() {
    let mut registry = Registry::new();
    registry.insert(mail_manifest().expect("mail"));
    registry.insert(files_manifest().expect("files"));
    assert!(registry.unresolved().is_empty());
    assert!(parse(MAIL_MANIFEST).is_ok() && parse(FILES_MANIFEST).is_ok());
}

#[test]
fn fake_router_installs_the_fixture_and_the_shipped_manifests() {
    let router = fake_router(AgentConfig::default()).expect("router");
    let state = router.state.lock().expect("lock");
    assert_eq!(state.registry.all().count(), 4);
    assert!(state.registry.unresolved().is_empty());
}

#[tokio::test]
async fn archive_then_undo_round_trips_through_the_apps_own_store() {
    let app = mail();
    let out = app
        .perform(invocation("mail.thread.archive", &["t1"]))
        .await
        .expect("archived");
    assert!(app.is_archived("t1"));
    let Undoable::Yes(token) = out.undo else {
        panic!("archive is undoable")
    };
    app.undo(
        token.clone(),
        Actor::User {
            via: AppName::parse("org.quire.Shell").expect("app"),
        },
    )
    .await
    .expect("undone");
    assert!(!app.is_archived("t1"));
    // The token is spent: a second undo says the change is gone.
    let again = app
        .undo(
            token,
            Actor::User {
                via: AppName::parse("org.quire.Shell").expect("app"),
            },
        )
        .await;
    assert_eq!(again, Err(UndoFault::Gone));
}

#[tokio::test]
async fn reading_a_thread_labels_the_body_untrusted() {
    let out = mail()
        .perform(invocation("mail.thread.read", &["t1"]))
        .await
        .expect("read");
    let value = out.value.expect("a value");
    assert_eq!(value.value, Value::Text("hello".into()));
    assert_eq!(value.label.integrity, prov::Integrity::Untrusted);
    assert!(value.label.sources.contains(&prov::Source::Mail));
}

#[tokio::test]
async fn a_missing_thread_is_not_found_and_delete_is_not_undoable() {
    let app = mail();
    let missing = app
        .perform(invocation("mail.thread.archive", &["nope"]))
        .await;
    assert!(matches!(missing, Err(AppRefusal::NotFound(_))));
    let gone = app
        .perform(invocation("mail.thread.delete", &["t1"]))
        .await
        .expect("deleted");
    assert_eq!(gone.undo, Undoable::No);
    assert!(!app.has_thread("t1"));
}

#[tokio::test]
async fn search_finds_subjects_and_suggest_finds_contacts() {
    let app = mail();
    assert_eq!(app.search("Receipts").await.len(), 1);
    assert!(app.search("zzz").await.is_empty());
    let ask = SuggestAsk {
        action: ActionRef {
            app: AppName::parse("org.quire.Mail").expect("app"),
            name: ActionName::parse("mail.message.send").expect("a"),
        },
        param: ParamName::parse("to").expect("p"),
        typed: "Acc".into(),
    };
    assert_eq!(app.suggest(ask).await.len(), 1);
}

#[tokio::test]
async fn files_move_and_undo() {
    let manifest = files_manifest().expect("files");
    let files = FakeFiles::new(manifest, space()).with_file("f1", "/home/p/a.pdf", "x");
    let app = AppName::parse("org.quire.Files").expect("app");
    let mut inv = invocation("files.file.move", &["f1"]);
    inv.target = TargetValue::Entities(vec![prov::EntityId {
        app,
        kind: prov::EntityKind::parse("files.file").expect("k"),
        key: prov::EntityKey::parse("f1").expect("k"),
    }]);
    inv.args.insert(
        ParamName::parse("to").expect("p"),
        prov::Labelled {
            value: Value::File(FileRef::parse("/home/p/docs").expect("f")),
            label: prov::Label::trusted_user(),
        },
    );
    let preview = files.dry_run(inv.clone()).await.expect("preview");
    assert!(matches!(preview, Preview::Moves(m) if m.len() == 1));
    let out = files.perform(inv).await.expect("moved");
    assert_eq!(files.path_of("f1").as_deref(), Some("/home/p/docs/f1"));
    let Undoable::Yes(token) = out.undo else {
        panic!("move is undoable")
    };
    files.undo(token, Actor::Unknown).await.expect("undone");
    assert_eq!(files.path_of("f1").as_deref(), Some("/home/p/a.pdf"));
}

fn review_request() -> action_review::ReviewRequest {
    use prov::{Integrity, SpaceId};
    action_review::ReviewRequest {
        space: SpaceId::parse("work").expect("space"),
        strictness: Strictness::Default,
        turns: vec![],
        proposed: action_review::ProposedAction {
            app: AppName::parse("org.quire.Mail").expect("app"),
            action: ActionName::parse("mail.thread.archive").expect("a"),
            label: LabelText::parse("Archive").expect("l"),
            effect: prov::Effect::UndoableWrite,
            kinds: BTreeSet::new(),
            count: porter_core::Count(1),
            args: vec![],
            lasting: Lasting::No,
        },
        labels: ArgLabels {
            per_arg: Default::default(),
            planner: Integrity::Trusted,
            saw: SessionSaw {
                private: Saw::NotSeen,
                untrusted: Saw::NotSeen,
            },
        },
        task_policy: None,
        history: vec![],
    }
}

#[tokio::test]
async fn the_scripted_reviewer_counts_its_calls_and_follows_its_queue() {
    let reviewer = ScriptedReviewer::queued(
        vec![Ok(ReviewVerdict::Allow), Err(ReviewError::Timeout)],
        ReviewMode::AlwaysAsk,
    );
    let request = review_request();
    assert_eq!(
        reviewer.review(Stage::Quick, &request).await,
        Ok(ReviewVerdict::Allow)
    );
    assert_eq!(
        reviewer.review(Stage::Deliberate, &request).await,
        Err(ReviewError::Timeout)
    );
    assert!(matches!(
        reviewer.review(Stage::SecondOpinion, &request).await,
        Ok(ReviewVerdict::Ask { .. })
    ));
    assert_eq!(reviewer.call_count(), 3);
    let stages: Vec<Stage> = reviewer.calls().into_iter().map(|(s, _)| s).collect();
    assert_eq!(
        stages,
        [Stage::Quick, Stage::Deliberate, Stage::SecondOpinion]
    );
    assert_eq!(
        ScriptedReviewer::always_allow()
            .review(Stage::Quick, &request)
            .await,
        Ok(ReviewVerdict::Allow)
    );
    assert_eq!(
        ScriptedReviewer::timing_out()
            .review(Stage::Quick, &request)
            .await,
        Err(ReviewError::Timeout)
    );
}

#[tokio::test]
async fn the_scripted_confirmer_pops_in_order_and_records_requests() {
    let receipt = |n: &str| prov::ConfirmReceipt {
        id: ConfirmId::parse(n).expect("id"),
        input: prov::InputProof::SheetFallback,
        at: UnixSeconds(1),
        covers: prov::Confidentiality::Secret,
    };
    let confirmer = ScriptedConfirmer::answering(vec![
        ConfirmAnswer::Allowed {
            scope: GrantScope::Once,
            receipt: receipt("c-1"),
        },
        ConfirmAnswer::Ended(ConfirmEnd::Refused),
    ]);
    let request = |id: &str| ConfirmRequest {
        id: ConfirmId::parse(id).expect("id"),
        space: space(),
        actor: Actor::Unknown,
        app: AppName::parse("org.quire.Mail").expect("app"),
        action: LabelText::parse("Send").expect("l"),
        effect: prov::Effect::Outbound,
        count: porter_core::Count(1),
        detail: ConfirmDetail::Plain,
        lines: vec![],
        why: vec![],
        taint: TaintNote::Clean,
        offer: ConfirmOffer::OnceOnly,
        gesture: Gesture::Press,
        anchor: Anchor::Centre,
        expires: Seconds(120),
    };
    use docket_core::Confirmer;
    assert!(matches!(
        confirmer.confirm(request("c-1")).await,
        ConfirmAnswer::Allowed { .. }
    ));
    assert_eq!(
        confirmer.confirm(request("c-2")).await,
        ConfirmAnswer::Ended(ConfirmEnd::Refused)
    );
    assert_eq!(
        confirmer.confirm(request("c-3")).await,
        ConfirmAnswer::Ended(ConfirmEnd::Dismissed)
    );
    confirmer
        .cancel(&ConfirmId::parse("c-3").expect("id"))
        .await;
    assert_eq!(confirmer.requests().len(), 3);
    assert_eq!(confirmer.cancelled().len(), 1);
}

#[test]
fn the_clock_is_fixed_and_the_sink_and_grants_record() {
    let clock = FixedClock(UnixSeconds(42));
    assert_eq!(
        (clock.now(), clock.now()),
        (UnixSeconds(42), UnixSeconds(42))
    );
    let sink = RecordingSink::new();
    sink.append(AuditRecord::Halt {
        at: UnixSeconds(1),
        scope: prov::SpaceScope::Any,
        cause: HaltCause::KillChord,
    });
    sink.append(AuditRecord::Halt {
        at: UnixSeconds(2),
        scope: prov::SpaceScope::Any,
        cause: HaltCause::StopKey,
    });
    assert_eq!(sink.records().len(), 2);
    assert!(MemoryGrants::new().grants().is_empty());
}

#[tokio::test]
async fn the_scripted_writer_and_reader_answer_and_count() {
    use docket_core::{PolicyWriter, Reader};
    let writer = ScriptedWriter::failing();
    let task = prov::TaskId::parse("t-1").expect("task");
    assert_eq!(
        writer.derive(&task, &[], &[], &space()).await,
        Err(ReviewError::Unavailable)
    );
    assert_eq!(writer.calls(), [(task, 0)]);
    let reader = ScriptedReader::answering(vec![Ok(Value::Integer(3))]);
    let ask = ReaderAsk {
        inputs: vec![],
        want: ValueSchema::Integer { min: 0, max: 9 },
        task: ReaderTask::Extract,
    };
    assert_eq!(
        reader.extract(ask.clone(), vec![]).await,
        Ok(Value::Integer(3))
    );
    assert_eq!(reader.extract(ask, vec![]).await, Err(ReaderError::Refused));
    assert_eq!(reader.asks().len(), 2);
}

#[tokio::test]
async fn a_client_reaches_the_router_in_process_behind_its_feature() {
    use docket_client::{InProcess, Intents};
    let router = fake_router(AgentConfig::default()).expect("router");
    router.seams.link.mail.add_thread(MailThread {
        key: "t1".into(),
        subject: "Hi".into(),
        from: "a@example.test".into(),
        body: "Hello".into(),
    });
    let router = std::sync::Arc::new(router);
    let launcher = CallerId {
        app: porter_core::AppId {
            name: AppName::parse("org.quire.Shell").expect("app"),
            isolation: porter_core::Isolation::Unsandboxed,
        },
        roles: BTreeSet::from([CallerRole::Launcher]),
    };
    let intents = Intents::over(InProcess::new(router.clone(), launcher));
    let end = intents
        .perform(
            CallRequest {
                action: ActionRef {
                    app: AppName::parse("org.quire.Mail").expect("app"),
                    name: ActionName::parse("mail.thread.archive").expect("action"),
                },
                target: TargetValue::Entities(vec![prov::EntityId {
                    app: AppName::parse("org.quire.Mail").expect("app"),
                    kind: prov::EntityKind::parse("mail.thread").expect("kind"),
                    key: prov::EntityKey::parse("t1").expect("key"),
                }]),
                args: Args::new(),
                origin: Origin::Launcher,
            },
            None,
            None,
        )
        .await
        .expect("a reply");
    assert!(end.is_ok(), "the person's own act ran: {end:?}");
    assert!(router.seams.link.mail.is_archived("t1"));
}
