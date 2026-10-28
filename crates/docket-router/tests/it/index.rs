//! The shadow index and search: the owner is the connection, epochs are checked, and titles
//! are labelled from their kind's declared trust.

use crate::support::*;
use docket_core::*;
use docket_fake::FakeSeams;
use docket_manifest::open_name;
use docket_router::Router;
use docket_vocab::{ActionRef, Args, CallRequest, CallerRole, Generation, Origin, TargetValue};
use docket_wire::{CallerId, IntentsReply, IntentsRequest, WireRefusal};
use prov::{EntityKey, EntityKind, Integrity, SpaceScope, UnixSeconds};

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

async fn found(router: &Router<FakeSeams>, text: &str) -> Vec<String> {
    let IntentsReply::Hits(hits) = ask(router, &launcher(), query(text)).await else {
        panic!("hits")
    };
    hits.iter()
        .map(|h| h.entity.id.key.as_str().to_owned())
        .collect()
}

async fn push(router: &Router<FakeSeams>, epoch: u64, entries: Vec<IndexEntry>) {
    let reset = IntentsRequest::IndexReset { epoch };
    assert_eq!(ask(router, &mail_caller(), reset).await, IntentsReply::Done);
    assert_eq!(
        ask(router, &mail_caller(), batch(epoch, entries)).await,
        IntentsReply::Done
    );
}

#[tokio::test]
async fn a_search_answers_from_the_index_and_asks_no_app() {
    use docket_fake::Answering;
    let router = router();
    router
        .seams
        .link
        .answer_from(&mail_app(), Answering::Stopped);
    router
        .seams
        .link
        .answer_from(&app("org.quire.Files"), Answering::Stopped);
    push(&router, 1, vec![entry("mail.thread", "t1", "Invoice")]).await;
    assert_eq!(found(&router, "invoice").await, ["t1"]);
    assert!(found(&router, "zzz").await.is_empty());
    assert!(
        router.seams.link.performed.lock().expect("lock").is_empty(),
        "a search performs nothing"
    );
}

#[tokio::test]
async fn a_preview_of_an_app_that_is_not_running_is_refused() {
    use docket_fake::Answering;
    let router = router();
    let files = app("org.quire.Files");
    router.seams.link.answer_from(&files, Answering::Stopped);
    let file = prov::EntityId {
        app: files,
        kind: EntityKind::parse("files.file").expect("kind"),
        key: EntityKey::parse("f1").expect("key"),
    };
    assert!(
        matches!(
            ask(&router, &launcher(), IntentsRequest::Preview(file)).await,
            IntentsReply::Refused(WireRefusal::Call(CallRefusal::AppUnavailable(_)))
        ),
        "nor is it started for a preview"
    );
}

#[tokio::test]
async fn hits_are_ranked_by_how_they_match_then_newest_first() {
    let router = router();
    let with =
        |key: &str, title: &str, subtitle: &str, keywords: &[&str], updated: i64| IndexEntry {
            subtitle: subtitle.into(),
            keywords: keywords.iter().map(|k| (*k).to_owned()).collect(),
            updated: UnixSeconds(updated),
            ..entry("mail.thread", key, title)
        };
    push(
        &router,
        1,
        vec![
            with("sub", "Else", "about REC", &[], 99),
            with("kw", "Other", "x", &["Recipe"], 98),
            with("has", "Lisbon receipts", "x", &[], 97),
            with("old", "Receipts", "x", &[], 1),
            with("new", "RECEIPTS 2", "x", &[], 5),
        ],
    )
    .await;
    assert_eq!(
        found(&router, "rec").await,
        ["new", "old", "has", "kw", "sub"]
    );
}

#[tokio::test]
async fn shipped_entries_survive_a_reset_and_a_pushed_one_wins_the_same_key() {
    let router = router();
    let shipped = vec![
        entry("mail.thread", "s1", "Shipped page"),
        entry("mail.thread", "t1", "Shipped twin"),
    ];
    router.ship_index(&mail_app(), shipped);
    assert_eq!(
        found(&router, "shipped").await,
        ["s1", "t1"],
        "found unpushed"
    );
    push(&router, 1, vec![entry("mail.thread", "t1", "Pushed twin")]).await;
    assert_eq!(found(&router, "shipped").await, ["s1"]);
    assert_eq!(found(&router, "twin").await, ["t1"]);
    let IntentsReply::Hits(hits) = ask(&router, &launcher(), query("twin")).await else {
        panic!("hits")
    };
    assert_eq!(hits[0].entity.title.value, "Pushed twin");
    ask(
        &router,
        &mail_caller(),
        IntentsRequest::IndexReset { epoch: 2 },
    )
    .await;
    // The reset wipes what was pushed and keeps what was shipped, so the shipped twin is back.
    assert_eq!(
        found(&router, "shipped").await,
        ["s1", "t1"],
        "a reset keeps them"
    );
    let IntentsReply::Hits(hits) = ask(&router, &launcher(), query("twin")).await else {
        panic!("hits")
    };
    assert_eq!(hits[0].entity.title.value, "Shipped twin");
}

#[tokio::test]
async fn shipped_entries_of_a_kind_that_is_not_indexed_are_dropped() {
    let router = router();
    let files = app("org.quire.Files");
    router.ship_index(&files, vec![entry("files.file", "f1", "Lisbon receipts")]);
    router.ship_index(
        &app("com.evil.Tool"),
        vec![entry("mail.thread", "e1", "Lisbon")],
    );
    assert!(found(&router, "lisbon").await.is_empty());
}

#[tokio::test]
async fn restored_entries_are_found_and_the_app_can_reset_as_usual() {
    let router = router();
    router.restore_index(
        &mail_app(),
        vec![
            entry("mail.thread", "t1", "Invoice"),
            entry("files.file", "f1", "Invoice file"),
        ],
    );
    assert_eq!(
        found(&router, "invoice").await,
        ["t1"],
        "only indexed kinds"
    );
    assert_eq!(router.pushed_entries(&mail_app()).len(), 1);
    push(&router, 1, vec![entry("mail.thread", "t2", "Digest")]).await;
    assert_eq!(found(&router, "invoice").await.len(), 0);
    assert_eq!(found(&router, "digest").await, ["t2"]);
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
            activation: None,
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
