//! The rules every `CheckpointStore` keeps, written once for each store's test to run (the
//! feature `testing`). Its checks panic by design. A store's test supplies a `Workbench`: a
//! folder the store works on and the means to change it from outside, as a person or an agent
//! would.

use crate::ids::WorkRoot;
use crate::store::{ApplyAsk, CheckpointStore, DropAsk, PlanAsk, StoreFault, TakeAsk};
use docket_core::{CheckpointId, PlanDigest, WorkPath};
use porter_core::{Count, UnixSeconds};
use prov::SessionId;

/// A folder under test and the hands to change it.
pub trait Workbench {
    /// The folder the store works on. It can keep points and starts with no files.
    fn root(&self) -> WorkRoot;
    /// A folder that cannot keep points, for a store that has such folders.
    fn without_history(&self) -> Option<WorkRoot>;
    /// Writes a file the folder does not ignore.
    fn write(&self, path: &WorkPath, content: &str);
    /// Writes a file the folder ignores.
    fn write_ignored(&self, path: &WorkPath, content: &str);
    /// Deletes a file.
    fn remove(&self, path: &WorkPath);
    /// A file's content now, ignored files included; none if it is missing.
    fn read(&self, path: &WorkPath) -> Option<String>;
}

fn path(text: &str) -> WorkPath {
    WorkPath::parse(text).expect("a contract path")
}

fn take(root: &WorkRoot, session: &SessionId, n: u32, max: u32) -> TakeAsk {
    TakeAsk {
        root: root.clone(),
        session: session.clone(),
        id: CheckpointId(n),
        at: UnixSeconds(100 * i64::from(n)),
        max: Count(max),
    }
}

fn plan(root: &WorkRoot, session: &SessionId, n: u32) -> PlanAsk {
    PlanAsk {
        root: root.clone(),
        session: session.clone(),
        id: CheckpointId(n),
    }
}

fn apply(root: &WorkRoot, session: &SessionId, n: u32, expect: PlanDigest) -> ApplyAsk {
    ApplyAsk {
        root: root.clone(),
        session: session.clone(),
        id: CheckpointId(n),
        expect,
    }
}

/// Runs every rule against `store` over `bench`.
pub async fn run<S: CheckpointStore, W: Workbench>(store: &S, bench: &W) {
    let root = bench.root();
    let session = SessionId::parse("s-contract").expect("session");
    let other = SessionId::parse("s-other").expect("session");
    let (a, b, c, ignored) = (
        path("a.txt"),
        path("dir/b.txt"),
        path("c.txt"),
        path("ignored.log"),
    );
    bench.write(&a, "one");
    bench.write(&b, "two");
    bench.write_ignored(&ignored, "noise");

    // A point is saved under the number and time asked, and listed.
    let saved = store.take(take(&root, &session, 1, 100)).await.expect("take");
    assert_eq!((saved.id, saved.at), (CheckpointId(1), UnixSeconds(100)));
    assert_eq!(store.held(&root, &session).await, Ok(vec![saved.clone()]));
    // Numbers are create-only, and sessions keep their own points.
    assert!(store.take(take(&root, &session, 1, 100)).await.is_err());
    assert_eq!(store.held(&root, &other).await, Ok(Vec::new()));

    // A folder that has not changed has nothing to restore.
    let same = store.plan(plan(&root, &session, 1)).await.expect("plan");
    assert!(same.changed.is_empty() && same.added.is_empty() && same.removed.is_empty());
    assert_eq!(store.plan(plan(&root, &session, 1)).await, Ok(same.clone()));

    // Changed, brought back and deleted files are told apart; ignored files are never listed.
    bench.write(&a, "ONE");
    bench.remove(&b);
    bench.write(&c, "three");
    bench.write_ignored(&ignored, "more noise");
    let change = store.plan(plan(&root, &session, 1)).await.expect("plan");
    assert_eq!(change.changed, vec![a.clone()]);
    assert_eq!(change.added, vec![b.clone()]);
    assert_eq!(change.removed, vec![c.clone()]);
    assert_ne!(change.digest, same.digest);
    // A plan writes nothing.
    assert_eq!(bench.read(&a).as_deref(), Some("ONE"));

    // A digest of anything else is refused and the folder is left as it was.
    let wrong = store
        .apply(apply(&root, &session, 1, PlanDigest("0".repeat(64))))
        .await;
    assert_eq!(wrong, Err(StoreFault::PlanStale));
    assert_eq!(bench.read(&a).as_deref(), Some("ONE"));
    assert_eq!(bench.read(&c).as_deref(), Some("three"));

    // The confirmed plan is carried out exactly, and the ignored file is not touched.
    let done = store
        .apply(apply(&root, &session, 1, change.digest.clone()))
        .await
        .expect("apply");
    assert_eq!(done, change);
    assert_eq!(bench.read(&a).as_deref(), Some("one"));
    assert_eq!(bench.read(&b).as_deref(), Some("two"));
    assert_eq!(bench.read(&c), None);
    assert_eq!(bench.read(&ignored).as_deref(), Some("more noise"));
    assert_eq!(store.plan(plan(&root, &session, 1)).await, Ok(same));
    // The same confirmation is stale once the folder has moved on.
    let late = store
        .apply(apply(&root, &session, 1, change.digest))
        .await;
    assert_eq!(late, Err(StoreFault::PlanStale));

    // A point that is not held is not a plan.
    assert_eq!(
        store.plan(plan(&root, &session, 9)).await,
        Err(StoreFault::NoSuchPoint)
    );
    assert_eq!(
        store.plan(plan(&root, &other, 1)).await,
        Err(StoreFault::NoSuchPoint)
    );

    // Too many files is refused and saves nothing.
    assert_eq!(
        store.take(take(&root, &session, 2, 1)).await,
        Err(StoreFault::TooLarge)
    );
    assert_eq!(store.held(&root, &session).await, Ok(vec![saved]));

    // Dropping forgets the points held and skips the others.
    let second = store.take(take(&root, &session, 2, 100)).await.expect("take");
    let dropping = DropAsk {
        root: root.clone(),
        session: session.clone(),
        ids: vec![CheckpointId(1), CheckpointId(9)],
    };
    assert_eq!(store.drop_points(dropping).await, Ok(Count(1)));
    assert_eq!(store.held(&root, &session).await, Ok(vec![second]));
    assert_eq!(
        store.plan(plan(&root, &session, 1)).await,
        Err(StoreFault::NoSuchPoint)
    );

    // A folder that cannot keep points says so.
    if let Some(bare) = bench.without_history() {
        assert_eq!(
            store.take(take(&bare, &session, 1, 100)).await,
            Err(StoreFault::NoHistory)
        );
    }
}
