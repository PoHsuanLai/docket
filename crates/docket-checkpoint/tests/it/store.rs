//! The stores of this crate keep the contract.

use crate::support::*;
use docket_checkpoint::{
    ApplyAsk, CheckpointStore, DropAsk, NoStore, PlanAsk, StoreFault, TakeAsk, WorkRoot,
};
use docket_core::{AbsPath, CheckpointId, PlanDigest};
use porter_core::{Count, UnixSeconds};
use prov::SessionId;

fn root(text: &str) -> WorkRoot {
    WorkRoot::new(AbsPath::parse(text).expect("path"))
}

#[cfg(feature = "testing")]
#[test]
fn the_memory_store_keeps_the_store_contract() {
    use docket_checkpoint::contract::{Workbench, run};
    use docket_checkpoint::{MemoryStore, Tracking};
    use docket_core::WorkPath;

    struct Bench<'a>(&'a MemoryStore);

    impl Workbench for Bench<'_> {
        fn root(&self) -> WorkRoot {
            root("/work/app")
        }
        fn without_history(&self) -> Option<WorkRoot> {
            let bare = root("/work/bare");
            self.0.lack_history(&bare);
            Some(bare)
        }
        fn write(&self, path: &WorkPath, content: &str) {
            self.0
                .write(&self.root(), path, content, Tracking::Followed);
        }
        fn write_ignored(&self, path: &WorkPath, content: &str) {
            self.0.write(&self.root(), path, content, Tracking::Ignored);
        }
        fn remove(&self, path: &WorkPath) {
            self.0.remove(&self.root(), path);
        }
        fn read(&self, path: &WorkPath) -> Option<String> {
            self.0.read(&self.root(), path)
        }
    }

    let store = MemoryStore::new();
    block_on(run(&store, &Bench(&store)));
}

#[test]
fn a_build_without_history_refuses_every_ask() {
    let (root, session) = (root("/work/app"), SessionId::parse("s-1").expect("session"));
    let id = CheckpointId(1);
    block_on(async {
        let take = TakeAsk {
            root: root.clone(),
            session: session.clone(),
            id,
            at: UnixSeconds(0),
            max: Count(10),
        };
        let plan = PlanAsk {
            root: root.clone(),
            session: session.clone(),
            id,
        };
        let apply = ApplyAsk {
            root: root.clone(),
            session: session.clone(),
            id,
            expect: PlanDigest::default(),
        };
        let drop = DropAsk {
            root: root.clone(),
            session: session.clone(),
            ids: vec![id],
        };
        let none = || Err(StoreFault::NoHistory);
        assert_eq!(NoStore.take(take).await.map(|_| ()), none());
        assert_eq!(NoStore.held(&root, &session).await.map(|_| ()), none());
        assert_eq!(NoStore.plan(plan).await.map(|_| ()), none());
        assert_eq!(NoStore.apply(apply).await.map(|_| ()), none());
        assert_eq!(NoStore.drop_points(drop).await.map(|_| ()), none());
    });
}
