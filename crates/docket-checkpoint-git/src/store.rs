//! `GitStore` as a `CheckpointStore`: take, list, plan, apply and forget points.

use crate::files::{delete, stay_inside};
use crate::git::{GitStore, strings};
use crate::names::{IndexKind, ref_name, session_prefix};
use crate::parse::{count_paths, held_points, named, object_id};
use crate::run::GitRun;
use docket_checkpoint::{
    ApplyAsk, CheckpointStore, DropAsk, PlanAsk, Saved, StoreFault, TakeAsk, TreeId, WorkRoot,
    plan_restore,
};
use docket_core::{RestorePlan, WorkPath};
use porter_core::Count;
use prov::SessionId;
use std::path::PathBuf;

/// Files written back per `checkout-index`.
const CHUNK: usize = 200;

/// The name and address every point is committed under.
const NAME: &str = "Docket";
const MAIL: &str = "docket@localhost";

fn authorship(at: i64) -> Vec<(String, String)> {
    // `@` marks raw seconds: without it git reads a small number as some other date form.
    let when = format!("@{} +0000", at.max(0));
    let var = |name: &str, value: &str| (name.to_owned(), value.to_owned());
    vec![
        var("GIT_AUTHOR_NAME", NAME),
        var("GIT_AUTHOR_EMAIL", MAIL),
        var("GIT_AUTHOR_DATE", &when),
        var("GIT_COMMITTER_NAME", NAME),
        var("GIT_COMMITTER_EMAIL", MAIL),
        var("GIT_COMMITTER_DATE", &when),
    ]
}

/// A blocking file job that failed to run or ran into a link or an error.
async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, StoreFault> + Send + 'static,
) -> Result<T, StoreFault> {
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|_| StoreFault::Failed)?
}

impl<R: GitRun> GitStore<R> {
    /// The points of `session` in the repository holding `root`, oldest first.
    async fn points(&self, root: &WorkRoot, session: &SessionId) -> Result<Vec<Saved>, StoreFault> {
        self.require_repository(root).await?;
        let args = vec![
            "for-each-ref".to_owned(),
            "--format=%(refname)%09%(tree)%09%(committerdate:unix)".to_owned(),
            session_prefix(session),
        ];
        let out = self.ok_with(root, args, &[], None).await?;
        Ok(held_points(session, &out))
    }

    /// Writes the files of `tree` named by `paths` into the folder, through a private index.
    async fn write_back(
        &self,
        root: &WorkRoot,
        session: &SessionId,
        tree: &str,
        paths: &[&WorkPath],
    ) -> Result<(), StoreFault> {
        if paths.is_empty() {
            return Ok(());
        }
        let env = self.index_env(session, IndexKind::Restoring).await?;
        self.ok_with(
            root,
            vec!["read-tree".to_owned(), tree.to_owned()],
            &env,
            None,
        )
        .await?;
        for chunk in paths.chunks(CHUNK) {
            let mut names = Vec::new();
            for path in chunk {
                names.extend_from_slice(path.as_str().as_bytes());
                names.push(0);
            }
            let args = strings(&["checkout-index", "-f", "-z", "--stdin"]);
            self.ok_with(root, args, &env, Some(names)).await?;
        }
        Ok(())
    }
}

impl<R: GitRun> CheckpointStore for GitStore<R> {
    async fn take(&self, ask: TakeAsk) -> Result<Saved, StoreFault> {
        let TakeAsk {
            root,
            session,
            id,
            at,
            max,
        } = ask;
        self.require_repository(&root).await?;
        let env = self.index_env(&session, IndexKind::Taken).await?;
        let listed = self
            .ok(
                &root,
                &["ls-files", "-o", "-c", "--exclude-standard", "-z"],
                &env,
            )
            .await?;
        if count_paths(&listed) > usize::try_from(max.0).unwrap_or(usize::MAX) {
            return Err(StoreFault::TooLarge);
        }
        self.ok(&root, &["add", "-A", "--", "."], &env).await?;
        let tree = object_id(&self.ok(&root, &["write-tree"], &env).await?)
            .map_err(|_| StoreFault::Failed)?;
        let message = format!("docket restore point {} of {}", id.0, session.as_str());
        let args = vec![
            "commit-tree".to_owned(),
            tree.clone(),
            "-m".to_owned(),
            message,
        ];
        let made = self.ok_with(&root, args, &authorship(at.0), None).await?;
        let commit = object_id(&made).map_err(|_| StoreFault::Failed)?;
        // Create-only: the all-zero old value means "the ref must not exist yet".
        let args = vec![
            "update-ref".to_owned(),
            ref_name(&session, id),
            commit.clone(),
            "0".repeat(commit.len()),
        ];
        self.ok_with(&root, args, &[], None).await?;
        Ok(Saved {
            id,
            at,
            tree: TreeId::new(tree),
        })
    }

    async fn held(&self, root: &WorkRoot, session: &SessionId) -> Result<Vec<Saved>, StoreFault> {
        self.points(root, session).await
    }

    async fn plan(&self, ask: PlanAsk) -> Result<RestorePlan, StoreFault> {
        self.require_repository(&ask.root).await?;
        let tree = self.point_tree(&ask.root, &ask.session, ask.id).await?;
        let then = self.listing_then(&ask.root, &tree).await?;
        let now = self.listing_now(&ask.root, &ask.session).await?;
        Ok(plan_restore(&now, &then))
    }

    async fn apply(&self, ask: ApplyAsk) -> Result<RestorePlan, StoreFault> {
        self.require_repository(&ask.root).await?;
        let tree = self.point_tree(&ask.root, &ask.session, ask.id).await?;
        let then = self.listing_then(&ask.root, &tree).await?;
        let now = self.listing_now(&ask.root, &ask.session).await?;
        let plan = plan_restore(&now, &then);
        if plan.digest != ask.expect {
            return Err(StoreFault::PlanStale);
        }
        let folder = PathBuf::from(ask.root.path().as_str());
        let everything: Vec<WorkPath> = plan
            .changed
            .iter()
            .chain(&plan.added)
            .chain(&plan.removed)
            .cloned()
            .collect();
        let (check_root, check_paths) = (folder.clone(), everything);
        blocking(move || {
            let refs: Vec<&WorkPath> = check_paths.iter().collect();
            stay_inside(&check_root, &refs).map_err(|_| StoreFault::Failed)
        })
        .await?;
        let (delete_root, gone) = (folder, plan.removed.clone());
        blocking(move || delete(&delete_root, &gone).map_err(|_| StoreFault::Failed)).await?;
        let wanted: Vec<&WorkPath> = plan.changed.iter().chain(&plan.added).collect();
        self.write_back(&ask.root, &ask.session, &tree, &wanted)
            .await?;
        Ok(plan)
    }

    async fn drop_points(&self, ask: DropAsk) -> Result<Count, StoreFault> {
        let held = self.points(&ask.root, &ask.session).await?;
        let mut dropped = 0u32;
        for id in named(&held, &ask.ids) {
            let args = vec![
                "update-ref".to_owned(),
                "-d".to_owned(),
                ref_name(&ask.session, id),
            ];
            self.ok_with(&ask.root, args, &[], None).await?;
            dropped = dropped.saturating_add(1);
        }
        Ok(Count(dropped))
    }
}
