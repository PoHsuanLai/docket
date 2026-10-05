//! A target in another Space: the shadow index says which Spaces an entity belongs to (an entity id
//! names none), so a call on a thing scoped elsewhere is cross-Space, and Cedar sends it to the
//! person even when it is a read.

mod support;

use docket_core::*;
use docket_router::{SpaceOf, relation_of};
use policy_point::SpaceRelation;
use prov::{EntityId, EntityKey, EntityKind, SpaceId, SpaceScope, UnixSeconds};
use std::collections::BTreeMap;
use support::*;

fn mail_caller() -> CallerId {
    caller("org.quire.Mail", CallerRole::App)
}

async fn index(
    router: &docket_router::Router<docket_fake::FakeSeams>,
    key: &str,
    scope: SpaceScope,
) {
    let reset = ask(
        router,
        &mail_caller(),
        IntentsRequest::IndexReset { epoch: 1 },
    )
    .await;
    assert_ne!(reset, IntentsReply::Refused(WireRefusal::NotAllowed));
    let push = ask(
        router,
        &mail_caller(),
        IntentsRequest::IndexPush(IndexBatch {
            epoch: 1,
            upserts: vec![IndexEntry {
                key: EntityKey::parse(key).expect("key"),
                kind: EntityKind::parse("mail.thread").expect("kind"),
                title: "Elsewhere".into(),
                subtitle: String::new(),
                keywords: vec![],
                updated: UnixSeconds(1),
                space: scope,
            }],
            removals: vec![],
        }),
    )
    .await;
    assert_eq!(push, IntentsReply::Done, "{push:?}");
}

#[tokio::test]
async fn cross_space_target_asks() {
    let router = router();
    ready(&router).await;
    index(&router, "t1", SpaceScope::Only(space("home"))).await;
    // t1 is the home Space's, the session is in work: a read, which never asks, now does.
    let outcome = perform(&router, call("mail.thread.read", &["t1"], vec![])).await;
    assert!(
        !router.seams.confirmer.requests().is_empty(),
        "a cross-Space target reaches the person: {outcome:?}"
    );
}

#[tokio::test]
async fn a_target_of_this_space_or_every_space_or_unknown_does_not_ask() {
    let router = router();
    ready(&router).await;
    index(&router, "t1", SpaceScope::Only(space("work"))).await;
    index(&router, "t2", SpaceScope::Any).await;
    for key in ["t1", "t2", "t-unknown"] {
        let _ = perform(&router, call("mail.thread.read", &[key], vec![])).await;
    }
    assert!(router.seams.confirmer.requests().is_empty());
}

#[tokio::test]
async fn an_mcp_client_is_denied_a_cross_space_target() {
    let router = router();
    ready(&router).await;
    index(&router, "t1", SpaceScope::Only(space("home"))).await;
    let mcp = caller("org.example.Claude", CallerRole::Mcp);
    let reply = ask(
        &router,
        &mcp,
        IntentsRequest::Perform {
            call: CallRequest {
                origin: Origin::Mcp,
                ..call("mail.thread.read", &["t1"], vec![])
            },
            session: None,
            parent_window: None,
        },
    )
    .await;
    let IntentsReply::Performed(end) = reply else {
        panic!("{reply:?}")
    };
    assert!(
        matches!(*end, Err(CallRefusal::Denied(_))),
        "an MCP client never reaches into another Space: {end:?}"
    );
}

struct Known(BTreeMap<EntityKey, SpaceScope>);

impl SpaceOf for Known {
    fn scope_of(&self, entity: &EntityId) -> Option<SpaceScope> {
        self.0.get(&entity.key).cloned()
    }
}

#[test]
fn the_relation_of_targets_to_the_session_is_pure_over_the_resolver() {
    let key = |k: &str| EntityKey::parse(k).expect("key");
    let known = Known(BTreeMap::from([
        (key("here"), SpaceScope::Only(space("work"))),
        (key("everywhere"), SpaceScope::Any),
        (key("there"), SpaceScope::Only(space("home"))),
    ]));
    let on = TargetKind::One(prov::EntityKind::parse("mail.thread").expect("kind"));
    let work: SpaceId = space("work");
    let relation = |keys: &[&str], on: &TargetKind| {
        let targets: Vec<EntityId> = keys.iter().map(|k| entity("mail.thread", k)).collect();
        relation_of(&known, &work, on, &targets)
    };
    assert_eq!(
        relation(&["here", "everywhere", "unknown"], &on),
        SpaceRelation::Same
    );
    assert_eq!(relation(&["here", "there"], &on), SpaceRelation::Other);
    assert_eq!(relation(&[], &TargetKind::Nothing), SpaceRelation::Unbound);
}
