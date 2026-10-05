//! The shadow index and search: the owner is the connection, epochs are checked, and titles
//! are labelled from their kind's declared trust.

mod support;

use docket_core::*;
use prov::{EntityKey, EntityKind, Integrity, SpaceScope, UnixSeconds};
use support::*;

fn mail_caller() -> CallerId {
    caller("org.quire.Mail", CallerRole::App)
}

fn entry(kind: &str, key: &str, title: &str) -> IndexEntry {
    IndexEntry {
        key: EntityKey::parse(key).expect("key"),
        kind: EntityKind::parse(kind).expect("kind"),
        title: title.into(),
        subtitle: "from Eve".into(),
        keywords: vec![],
        updated: UnixSeconds(1),
        space: SpaceScope::Only(space("work")),
    }
}

fn batch(epoch: u64, upserts: Vec<IndexEntry>) -> IntentsRequest {
    IntentsRequest::IndexPush(IndexBatch {
        epoch,
        upserts,
        removals: vec![],
    })
}

fn query(text: &str) -> IntentsRequest {
    IntentsRequest::Search(SearchAsk {
        text: text.into(),
        scope: SearchScope::Everything,
        generation: Generation(1),
    })
}

#[tokio::test]
async fn a_push_needs_a_reset_in_the_same_epoch() {
    let router = router();
    let push = batch(7, vec![entry("mail.thread", "t9", "Lisbon receipts")]);
    assert_eq!(
        ask(&router, &mail_caller(), push.clone()).await,
        IntentsReply::Refused(WireRefusal::Malformed),
        "nothing was reset, so the app is asked to start an epoch"
    );
    assert_eq!(
        ask(
            &router,
            &mail_caller(),
            IntentsRequest::IndexReset { epoch: 7 }
        )
        .await,
        IntentsReply::Done
    );
    assert_eq!(ask(&router, &mail_caller(), push).await, IntentsReply::Done);
    assert_eq!(
        ask(&router, &mail_caller(), batch(8, vec![])).await,
        IntentsReply::Refused(WireRefusal::Malformed),
        "a push in another epoch is refused"
    );
}

#[tokio::test]
async fn an_app_without_a_manifest_cannot_index_and_only_the_owner_pushes() {
    let router = router();
    let stranger = caller("com.evil.Tool", CallerRole::App);
    assert_eq!(
        ask(&router, &stranger, IntentsRequest::IndexReset { epoch: 1 }).await,
        IntentsReply::Refused(WireRefusal::NotAllowed)
    );
    let launcher_push = ask(
        &router,
        &launcher(),
        IntentsRequest::IndexReset { epoch: 1 },
    )
    .await;
    assert_eq!(
        launcher_push,
        IntentsReply::Refused(WireRefusal::NotAllowed),
        "the role table: apps only"
    );
}

#[tokio::test]
async fn found_titles_carry_the_trust_their_kind_declares() {
    let router = router();
    ask(
        &router,
        &mail_caller(),
        IntentsRequest::IndexReset { epoch: 1 },
    )
    .await;
    ask(
        &router,
        &mail_caller(),
        batch(
            1,
            vec![
                entry("mail.thread", "t9", "Lisbon receipts"),
                entry("mail.contact", "c9", "Lisbon Ltd"),
            ],
        ),
    )
    .await;
    let IntentsReply::Hits(hits) = ask(&router, &launcher(), query("lisbon")).await else {
        panic!("hits")
    };
    let thread = hits
        .iter()
        .find(|h| h.entity.id.kind.as_str() == "mail.thread")
        .expect("thread");
    assert_eq!(
        thread.entity.title.label.integrity,
        Integrity::Untrusted,
        "a mail subject is somebody else's"
    );
    // The contact kind is not indexed by the fixture manifest: the push of it was dropped.
    assert!(
        hits.iter()
            .all(|h| h.entity.id.kind.as_str() != "mail.contact"),
        "{hits:?}"
    );
}

#[tokio::test]
async fn a_removal_drops_a_thing_and_a_reset_forgets_everything() {
    let router = router();
    ask(
        &router,
        &mail_caller(),
        IntentsRequest::IndexReset { epoch: 1 },
    )
    .await;
    ask(
        &router,
        &mail_caller(),
        batch(1, vec![entry("mail.thread", "t9", "Lisbon receipts")]),
    )
    .await;
    ask(
        &router,
        &mail_caller(),
        IntentsRequest::IndexPush(IndexBatch {
            epoch: 1,
            upserts: vec![],
            removals: vec![EntityKey::parse("t9").expect("key")],
        }),
    )
    .await;
    let IntentsReply::Hits(after_removal) = ask(&router, &launcher(), query("lisbon")).await else {
        panic!()
    };
    assert!(
        after_removal
            .iter()
            .all(|h| h.entity.id.key.as_str() != "t9")
    );
    ask(
        &router,
        &mail_caller(),
        batch(1, vec![entry("mail.thread", "t9", "Lisbon receipts")]),
    )
    .await;
    ask(
        &router,
        &mail_caller(),
        IntentsRequest::IndexReset { epoch: 2 },
    )
    .await;
    let IntentsReply::Hits(after_reset) = ask(&router, &launcher(), query("lisbon")).await else {
        panic!()
    };
    assert!(after_reset.iter().all(|h| h.entity.id.key.as_str() != "t9"));
}

#[tokio::test]
async fn search_also_asks_the_apps_for_what_they_do_not_index() {
    let router = router();
    // The files kind of the fixture is searched live; the fake finds nothing by this word, the
    // mail threads are found by their subject through the shadow index only.
    let IntentsReply::Hits(hits) = ask(&router, &launcher(), query("Invoice")).await else {
        panic!()
    };
    assert!(
        hits.iter()
            .all(|h| h.entity.id.kind.as_str() != "mail.thread"),
        "mail is indexed, not searched live"
    );
    let IntentsReply::Manifests(manifests) = ask(
        &router,
        &caller("org.quire.Anything", CallerRole::App),
        IntentsRequest::Manifests,
    )
    .await
    else {
        panic!("manifests")
    };
    assert_eq!(manifests.len(), 4);
}

#[tokio::test]
async fn a_hit_is_opened_by_its_kinds_open_action_performed_as_the_launcher() {
    let router = router();
    ask(
        &router,
        &mail_caller(),
        IntentsRequest::IndexReset { epoch: 1 },
    )
    .await;
    ask(
        &router,
        &mail_caller(),
        batch(1, vec![entry("mail.thread", "t1", "Invoice")]),
    )
    .await;
    let IntentsReply::Hits(hits) = ask(&router, &launcher(), query("invoice")).await else {
        panic!("hits")
    };
    let id = hits[0].entity.id.clone();
    // The convention: the action is `<kind>.open` of the hit's own app, on the hit.
    let open = CallRequest {
        action: ActionRef {
            app: id.app.clone(),
            name: open_name(&id.kind).expect("name"),
        },
        target: TargetValue::Entities(vec![id]),
        args: Args::new(),
        origin: Origin::Launcher,
    };
    let reply = ask(
        &router,
        &launcher(),
        IntentsRequest::Perform {
            call: open,
            session: None,
            parent_window: None,
        },
    )
    .await;
    assert!(
        matches!(reply, IntentsReply::Performed(ref end) if end.is_ok()),
        "{reply:?}"
    );
    assert_eq!(router.seams.link.mail.opened(), vec!["t1".to_string()]);
    assert!(
        router.seams.confirmer.requests().is_empty(),
        "opening reads: nothing asks"
    );
}
