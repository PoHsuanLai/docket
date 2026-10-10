//! The git store in scratch repositories: the store contract, and the person's own history left
//! exactly as it was.

use crate::scratch::{Scratch, root};
use docket_checkpoint::contract::{Workbench, run};
use docket_checkpoint::{ApplyAsk, CheckpointStore, PlanAsk, StoreFault, TakeAsk, WorkRoot};
use docket_checkpoint_git::ref_name;
use docket_core::{CheckpointId, WorkPath};
use porter_core::{Count, UnixSeconds};
use prov::SessionId;
use std::path::{Path, PathBuf};

fn path(text: &str) -> WorkPath {
    WorkPath::parse(text).expect("path")
}

fn session() -> SessionId {
    SessionId::parse("s-git").expect("session")
}

fn take(root: &WorkRoot, n: u32) -> TakeAsk {
    TakeAsk {
        root: root.clone(),
        session: session(),
        id: CheckpointId(n),
        at: UnixSeconds(1_000 + i64::from(n)),
        max: Count(1000),
    }
}

struct Bench {
    repo: PathBuf,
    bare: PathBuf,
}

impl Workbench for Bench {
    fn root(&self) -> WorkRoot {
        root(&self.repo)
    }
    fn without_history(&self) -> Option<WorkRoot> {
        Some(root(&self.bare))
    }
    fn write(&self, path: &WorkPath, content: &str) {
        self.write_ignored(path, content);
    }
    fn write_ignored(&self, path: &WorkPath, content: &str) {
        let file = self.repo.join(path.as_str());
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("folders");
        std::fs::write(file, content).expect("write");
    }
    fn remove(&self, path: &WorkPath) {
        std::fs::remove_file(self.repo.join(path.as_str())).expect("remove");
    }
    fn read(&self, path: &WorkPath) -> Option<String> {
        std::fs::read_to_string(self.repo.join(path.as_str())).ok()
    }
}

#[tokio::test]
async fn the_git_store_keeps_the_store_contract() {
    let Some(scratch) = Scratch::new() else {
        return;
    };
    let bare = scratch.path("bare");
    std::fs::create_dir_all(&bare).expect("bare folder");
    let bench = Bench {
        repo: scratch.repo("repo"),
        bare,
    };
    run(&scratch.store(), &bench).await;
}

/// Everything of the person's that a restore point must not touch, read the way a person would.
fn their_history(scratch: &Scratch, repo: &Path) -> Vec<String> {
    let mut state = vec![
        scratch.git(
            repo,
            &[
                "--no-optional-locks",
                "status",
                "--porcelain=v2",
                "--branch",
            ],
        ),
        scratch.git(repo, &["rev-parse", "HEAD"]),
        scratch.git(
            repo,
            &["for-each-ref", "refs/heads", "refs/tags", "refs/stash"],
        ),
        scratch.git(repo, &["stash", "list"]),
        scratch.git(repo, &["diff", "--cached"]),
    ];
    let index = std::fs::read(repo.join(".git/index")).expect("the index");
    state.push(format!("{index:?}"));
    state
}

#[tokio::test]
async fn taking_and_planning_leave_the_persons_index_head_branches_and_stash_as_they_were() {
    let Some(scratch) = Scratch::new() else {
        return;
    };
    let repo = scratch.repo("repo");
    std::fs::write(repo.join("tracked.txt"), "v1").expect("write");
    scratch.git(&repo, &["add", "tracked.txt"]);
    scratch.git(&repo, &["commit", "-q", "-m", "first"]);
    std::fs::write(repo.join("tracked.txt"), "v2").expect("write");
    scratch.git(&repo, &["stash", "push", "-q"]);
    std::fs::write(repo.join("staged.txt"), "staged").expect("write");
    scratch.git(&repo, &["add", "staged.txt"]);
    std::fs::write(repo.join("tracked.txt"), "v3").expect("write");
    std::fs::write(repo.join("loose.txt"), "loose").expect("write");
    let before = their_history(&scratch, &repo);

    let (store, there) = (scratch.store(), root(&repo));
    store.take(take(&there, 1)).await.expect("take");
    std::fs::write(repo.join("loose.txt"), "changed").expect("write");
    let plan = store
        .plan(PlanAsk {
            root: there.clone(),
            session: session(),
            id: CheckpointId(1),
        })
        .await
        .expect("plan");

    assert_eq!(plan.changed, vec![path("loose.txt")]);
    // loose.txt was edited after the point; its edit is the only difference to the status.
    let after = their_history(&scratch, &repo);
    assert!(after[0].contains("loose.txt"));
    assert_eq!(before[1..], after[1..]);
    let points = scratch.git(&repo, &["for-each-ref", "refs/docket"]);
    assert!(points.contains(&ref_name(&session(), CheckpointId(1))));
    // The point holds the staged, edited and untracked files, none of them as the index has them.
    let tree = scratch.git(
        &repo,
        &[
            "ls-tree",
            "-r",
            "--name-only",
            "refs/docket/checkpoints/s-git/1",
        ],
    );
    assert_eq!(tree, "loose.txt\nstaged.txt\ntracked.txt\n");
}

#[tokio::test]
async fn a_workspace_below_the_top_of_a_repository_is_saved_and_restored_by_its_own_paths() {
    let Some(scratch) = Scratch::new() else {
        return;
    };
    let repo = scratch.repo("repo");
    let work = repo.join("sub");
    std::fs::create_dir_all(&work).expect("sub");
    std::fs::write(work.join("a.txt"), "one").expect("write");
    std::fs::write(repo.join("outside.txt"), "not ours").expect("write");
    let (store, there) = (scratch.store(), root(&work));
    store.take(take(&there, 1)).await.expect("take");
    std::fs::write(work.join("a.txt"), "two").expect("write");
    std::fs::write(repo.join("outside.txt"), "also changed").expect("write");
    let ask = PlanAsk {
        root: there.clone(),
        session: session(),
        id: CheckpointId(1),
    };
    let plan = store.plan(ask).await.expect("plan");
    assert_eq!(plan.changed, vec![path("a.txt")]);
    assert!(plan.added.is_empty() && plan.removed.is_empty());
    let restore = ApplyAsk {
        root: there,
        session: session(),
        id: CheckpointId(1),
        expect: plan.digest,
    };
    store.apply(restore).await.expect("apply");
    assert_eq!(
        std::fs::read_to_string(work.join("a.txt")).expect("read"),
        "one"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("outside.txt")).expect("read"),
        "also changed"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_restore_does_not_write_through_a_link_that_took_the_place_of_a_folder() {
    let Some(scratch) = Scratch::new() else {
        return;
    };
    let repo = scratch.repo("repo");
    std::fs::create_dir_all(repo.join("dir")).expect("dir");
    std::fs::write(repo.join("dir/f.txt"), "inside").expect("write");
    let (store, there) = (scratch.store(), root(&repo));
    store.take(take(&there, 1)).await.expect("take");
    let outside = scratch.path("outside");
    std::fs::create_dir_all(&outside).expect("outside");
    std::fs::write(outside.join("f.txt"), "theirs").expect("write");
    std::fs::remove_dir_all(repo.join("dir")).expect("remove");
    std::os::unix::fs::symlink(&outside, repo.join("dir")).expect("link");
    let plan = store
        .plan(PlanAsk {
            root: there.clone(),
            session: session(),
            id: CheckpointId(1),
        })
        .await
        .expect("plan");
    let restore = ApplyAsk {
        root: there,
        session: session(),
        id: CheckpointId(1),
        expect: plan.digest,
    };
    assert_eq!(store.apply(restore).await, Err(StoreFault::Failed));
    assert_eq!(
        std::fs::read_to_string(outside.join("f.txt")).expect("read"),
        "theirs"
    );
    assert!(repo.join("dir").is_symlink());
}
