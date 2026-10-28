//! A folder whose `.git` the agent wrote: hooks, filter drivers and a file-system monitor planted
//! in a scratch repository must never run when the store takes, plans, restores and drops points.

use crate::scratch::{Scratch, root};
use docket_checkpoint::{ApplyAsk, CheckpointStore, DropAsk, PlanAsk, TakeAsk};
use docket_core::CheckpointId;
use porter_ids::{Count, UnixSeconds};
use prov::SessionId;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn script(path: &Path, marker: &Path) {
    let body = format!("#!/bin/sh\ntouch '{}'\ncat >/dev/null\n", marker.display());
    std::fs::write(path, body).expect("script");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("mode");
}

#[tokio::test]
async fn a_hook_a_filter_and_a_monitor_planted_in_the_folder_never_run() {
    let Some(scratch) = Scratch::new() else {
        return;
    };
    let repo = scratch.repo("repo");
    let (hook, filter, monitor) = (
        scratch.path("ran-hook"),
        scratch.path("ran-filter"),
        scratch.path("ran-monitor"),
    );
    script(&repo.join(".git/hooks/reference-transaction"), &hook);
    script(&scratch.path("monitor.sh"), &monitor);
    let config = format!(
        "[filter \"x\"]\n\tclean = \"touch {}; cat\"\n[core]\n\tfsmonitor = {}\n",
        filter.display(),
        scratch.path("monitor.sh").display()
    );
    let old = std::fs::read_to_string(repo.join(".git/config")).expect("config");
    std::fs::write(repo.join(".git/config"), format!("{old}{config}")).expect("config");
    std::fs::write(repo.join(".gitattributes"), "* filter=x\n").expect("attributes");
    std::fs::write(repo.join("a.txt"), "one").expect("write");

    // The plant works: git run by hand in this repository runs the filter and the hook.
    scratch.git(&repo, &["add", "-A"]);
    scratch.git(&repo, &["commit", "-q", "--allow-empty", "-m", "control"]);
    assert!(filter.exists() && hook.exists(), "the plant must work");
    std::fs::remove_file(&filter).expect("reset");
    std::fs::remove_file(&hook).expect("reset");
    let _ = std::fs::remove_file(&monitor);

    let (store, there) = (scratch.store(), root(&repo));
    let session = SessionId::parse("s-hostile").expect("session");
    let ask = |n: u32| TakeAsk {
        root: there.clone(),
        session: session.clone(),
        id: CheckpointId(n),
        at: UnixSeconds(10),
        max: Count(100),
    };
    store.take(ask(1)).await.expect("take");
    std::fs::write(repo.join("a.txt"), "two").expect("write");
    let plan = store
        .plan(PlanAsk {
            root: there.clone(),
            session: session.clone(),
            id: CheckpointId(1),
        })
        .await
        .expect("plan");
    store
        .apply(ApplyAsk {
            root: there.clone(),
            session: session.clone(),
            id: CheckpointId(1),
            expect: plan.digest,
        })
        .await
        .expect("apply");
    store.take(ask(2)).await.expect("take again");
    let dropped = DropAsk {
        root: there.clone(),
        session: session.clone(),
        ids: vec![CheckpointId(1)],
    };
    store.drop_points(dropped).await.expect("drop");

    for (marker, what) in [(&hook, "hook"), (&filter, "filter"), (&monitor, "monitor")] {
        assert!(!marker.exists(), "the planted {what} ran");
    }
}
