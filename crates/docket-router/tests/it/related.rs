//! Relations: a planner holding a thing gets a related thing of another kind as a handle, through
//! the gate, labelled by what the relation says and never better than where it was found.

use crate::support::*;
use docket_core::*;
use docket_fake::FakeSeams;
use docket_router::{GrantStore, HandleValue, Router};
use porter_core::consent::{Decision, Grant, GrantScope, Usage};
use prov::{DataClass, Integrity, SessionId};

fn related(kind: &str, key: &str, relation: &str) -> CallRequest {
    let owner = app(if kind.starts_with("files") {
        "org.quire.Files"
    } else {
        "org.quire.Mail"
    });
    CallRequest {
        action: ActionRef {
            app: owner.clone(),
            name: prov::ActionName::parse(&format!("{kind}.related")).expect("action"),
        },
        target: TargetValue::Entities(vec![prov::EntityId {
            app: owner,
            kind: prov::EntityKind::parse(kind).expect("kind"),
            key: prov::EntityKey::parse(key).expect("key"),
        }]),
        args: [(
            param("relation"),
            prov::Labelled {
                value: Value::Choice(ChoiceId::parse(relation).expect("choice")),
                label: trusted(),
            },
        )]
        .into(),
        origin: Origin::Companion,
    }
}

fn grant_files(router: &Router<FakeSeams>) {
    router.seams.grants.record(Grant {
        id: porter_core::GrantId::parse("g-files").expect("grant"),
        key: ActionGrantKey {
            caller: GrantCaller::Companion,
            owner: app("org.quire.Files"),
            target: GrantTarget::App,
            class: DataClass::Files,
            usage: Usage::Interactive,
            space: prov::SpaceScope::Only(space("work")),
        },
        decision: Decision::Allow,
        scope: GrantScope::Always,
        at: prov::UnixSeconds(0),
    });
}

/// The handle a related call returned, and what the session holds for it.
fn held(
    router: &Router<FakeSeams>,
    session: &SessionId,
    outcome: &Outcome,
) -> (Handle, Label, HandleValue) {
    let Some(Labelled {
        value: Value::List(items),
        ..
    }) = &outcome.value
    else {
        panic!("a list of handles: {:?}", outcome.value)
    };
    let [Value::Handle(h)] = items.as_slice() else {
        panic!("one handle: {items:?}")
    };
    let st = router.state.lock().expect("lock");
    let entry = st.sessions[session].handles.value(*h).expect("held");
    (*h, entry.label.clone(), entry.value.clone())
}

use prov::Label;
use prov::Labelled;

async fn session_of(router: &Router<FakeSeams>) -> SessionId {
    let s = ready(router).await;
    grant_files(router);
    s.session
}

#[tokio::test]
async fn a_relation_is_resolved_through_the_app_and_comes_back_as_a_handle() {
    let router = router();
    let session = session_of(&router).await;
    let outcome = perform(&router, related("mail.thread", "t1", "from"))
        .await
        .expect("ran");
    let (_, _, value) = held(&router, &session, &outcome);
    assert_eq!(
        value,
        HandleValue::Entity(entity("mail.contact", "eve@evil.test")),
        "the sender is a thing of kind contact, not text"
    );
    assert!(
        router.seams.sink.records().iter().any(|r| matches!(
            r,
            AuditRecord::Call { action, end: CallEnd::Done, .. }
                if action.name.as_str() == "mail.thread.related"
        )),
        "the app was asked to resolve it, and the call is on the record"
    );
}

#[tokio::test]
async fn a_saved_contact_is_named_by_its_own_key() {
    let router = router();
    let session = session_of(&router).await;
    router.seams.link.mail.add_thread(docket_fake::MailThread {
        key: "t3".into(),
        subject: "Receipts".into(),
        from: "accounting@example.test".into(),
        body: "attached".into(),
    });
    let outcome = perform(&router, related("mail.thread", "t3", "from"))
        .await
        .expect("ran");
    let (_, _, value) = held(&router, &session, &outcome);
    assert_eq!(value, HandleValue::Entity(entity("mail.contact", "c1")));
}

#[tokio::test]
async fn what_a_relation_takes_from_somebody_elses_words_is_untrusted_whatever_the_app_says() {
    let router = router();
    let session = session_of(&router).await;
    // The fake labels its answer as the app's own, as a careless app would.
    let outcome = perform(&router, related("mail.thread", "t1", "from"))
        .await
        .expect("ran");
    let answered = outcome.value.as_ref().expect("value");
    assert_eq!(answered.label.integrity, Integrity::Untrusted);
    let (_, label, _) = held(&router, &session, &outcome);
    assert_eq!(label.integrity, Integrity::Untrusted);
    assert!(label.sources.contains(&prov::Source::Mail), "{label:?}");
    let st = router.state.lock().expect("lock");
    assert_eq!(
        st.sessions[&session].saw.untrusted,
        Saw::Seen,
        "the planner is now one that has been shown untrusted things"
    );
}

#[tokio::test]
async fn a_relation_the_app_decides_alone_stays_trusted() {
    let router = router();
    let session = session_of(&router).await;
    router
        .seams
        .link
        .files
        .add_file("f1", "/home/a.txt", "words");
    router.seams.link.files.add_owner("f1", "ana");
    let outcome = perform(&router, related("files.file", "f1", "owner"))
        .await
        .expect("ran");
    let (_, label, _) = held(&router, &session, &outcome);
    assert_eq!(label.integrity, Integrity::Trusted);
}

#[tokio::test]
async fn a_thing_found_through_untrusted_content_is_untrusted_even_by_a_trusted_relation() {
    let router = router();
    let session = session_of(&router).await;
    router
        .seams
        .link
        .files
        .add_file("f1", "/home/a.txt", "words");
    router.seams.link.files.add_owner("f1", "ana");
    let file = {
        let mut st = router.state.lock().expect("lock");
        st.sessions
            .get_mut(&session)
            .expect("session")
            .handles
            .mint_entity(
                prov::EntityId {
                    app: app("org.quire.Files"),
                    kind: prov::EntityKind::parse("files.file").expect("kind"),
                    key: prov::EntityKey::parse("f1").expect("key"),
                },
                mail_label("work"),
            )
    };
    let mut call = related("files.file", "f1", "owner");
    call.target = TargetValue::Handles(vec![file]);
    let outcome = perform(&router, call).await.expect("ran");
    let (_, label, _) = held(&router, &session, &outcome);
    assert_eq!(
        label.integrity,
        Integrity::Untrusted,
        "the owner of a file found in untrusted words is no better than where it was found"
    );
}

#[tokio::test]
async fn a_recipient_chosen_through_a_relation_asks_like_any_untrusted_recipient() {
    let router = router();
    let session = session_of(&router).await;
    let outcome = perform(&router, related("mail.thread", "t1", "from"))
        .await
        .expect("ran");
    let (sender, _, _) = held(&router, &session, &outcome);
    let send = call(
        "mail.message.send",
        &[],
        vec![
            ("to", Value::Handle(sender)),
            ("body", Value::Text("tidy".into())),
        ],
    );
    let refused = perform(&router, send)
        .await
        .expect_err("asked, then dismissed");
    assert_eq!(refused, CallRefusal::Unconfirmed(ConfirmEnd::Dismissed));
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
        "the sender is drawn quoted, as somebody else's words"
    );
}

#[tokio::test]
async fn a_relation_the_kind_does_not_declare_is_a_bad_argument() {
    let router = router();
    session_of(&router).await;
    let refused = perform(&router, related("mail.thread", "t1", "owner"))
        .await
        .expect_err("refused");
    assert!(
        matches!(refused, CallRefusal::BadArgs { .. }),
        "{refused:?}"
    );
    assert!(
        !router.seams.sink.records().iter().any(|r| matches!(
            r,
            AuditRecord::Call { end: CallEnd::Done, action, .. }
                if action.name.as_str() == "mail.thread.related"
        )),
        "the app was never asked"
    );
}

#[tokio::test]
async fn resolving_a_relation_is_a_read_of_the_classes_of_both_kinds() {
    let router = router();
    let st = router.state.lock().expect("lock");
    let decl = st
        .registry
        .action(&action("mail.thread.related"))
        .expect("derived action");
    assert_eq!(decl.effect, prov::Effect::Read);
    assert_eq!(
        decl.classes,
        [DataClass::Mail, DataClass::Contacts].into_iter().collect()
    );
}

#[test]
fn a_manifest_file_declares_a_relation_and_the_registry_checks_the_kind_it_names() {
    let text = docket_fake::MAIL_MANIFEST;
    assert!(text.contains("[[entities.relations]]"));
    let mail = docket_router::parse(text).expect("the mail fixture parses and validates");
    let thread = mail
        .manifest()
        .entities
        .iter()
        .find(|e| e.kind.as_str() == "mail.thread")
        .expect("thread");
    let [from] = thread.relations.as_slice() else {
        panic!("one relation: {:?}", thread.relations)
    };
    assert_eq!(from.name.as_str(), "from");
    assert_eq!(from.to.as_str(), "mail.contact");
    assert_eq!(from.many, Cardinality::One);
    assert!(matches!(
        from.trust,
        TitleTrust::ThirdParty(prov::Source::Mail)
    ));

    // A relation to a kind no installed app declares is the registry's to refuse.
    let mut registry = docket_router::Registry::new();
    registry.insert(mail.clone());
    assert!(registry.unresolved().is_empty());
    let dangling = text.replace("to = \"mail.contact\"", "to = \"mail.nobody\"");
    let mut registry = docket_router::Registry::new();
    registry.insert(docket_router::parse(&dangling).expect("valid on its own"));
    assert!(
        registry.unresolved().iter().any(|e| matches!(
            e,
            ManifestError::UnknownKind { kind, .. } if kind.as_str() == "mail.nobody"
        )),
        "{:?}",
        registry.unresolved()
    );
}
