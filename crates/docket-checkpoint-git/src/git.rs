//! `GitStore`: the commands it runs and the listings it reads. The trait impl is in `store.rs`.

use crate::names::{IndexKind, index_file, ref_name};
use crate::parse::{object_id, staged_listing, tree_listing};
use crate::run::{GitCommand, GitOut, GitRun};
use docket_checkpoint::{StoreFault, TreeListing, WorkRoot};
use docket_core::{AbsPath, CheckpointId};
use prov::SessionId;

/// Options before every command: no background refresh of the person's index, no file-system
/// monitor hook, no signing.
const BEFORE: [&str; 6] = [
    "--no-optional-locks",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "commit.gpgsign=false",
    "--no-pager",
];

/// Restore points in the git repository that holds the folder.
///
/// Every point is a commit with no parent, made from a private index (a file under `index_dir`,
/// one set per session), kept under `refs/docket/checkpoints/<session>/<n>`. The person's own
/// index, HEAD, branches and stash are never named; only `apply` writes files of the folder.
#[derive(Debug)]
pub struct GitStore<R: GitRun> {
    pub(crate) run: R,
    index_dir: AbsPath,
    env: Vec<(String, String)>,
}

impl<R: GitRun> GitStore<R> {
    /// A store that runs git through `run` and keeps its private indexes in `index_dir`, a folder
    /// of docket's own (never inside a repository's `.git`).
    pub fn new(run: R, index_dir: AbsPath) -> Self {
        Self {
            run,
            index_dir,
            env: Vec::new(),
        }
    }

    /// Variables every command runs with, such as a `HOME` that is not the person's (tests).
    pub fn with_env(mut self, env: Vec<(String, String)>) -> Self {
        self.env = env;
        self
    }

    /// One command, whatever its status.
    pub(crate) async fn exec(
        &self,
        root: &WorkRoot,
        args: Vec<String>,
        env: &[(String, String)],
        stdin: Option<Vec<u8>>,
    ) -> Result<GitOut, crate::run::GitFault> {
        let mut all: Vec<String> = BEFORE.iter().map(|part| (*part).to_owned()).collect();
        all.extend(args);
        let mut vars = self.env.clone();
        vars.extend(env.iter().cloned());
        self.run
            .run(GitCommand {
                cwd: root.path().clone(),
                args: all,
                env: vars,
                stdin,
            })
            .await
    }

    /// One command that must end with success; what it printed.
    pub(crate) async fn ok(
        &self,
        root: &WorkRoot,
        args: &[&str],
        env: &[(String, String)],
    ) -> Result<Vec<u8>, StoreFault> {
        self.ok_with(root, strings(args), env, None).await
    }

    /// `ok` with owned arguments and standard input.
    pub(crate) async fn ok_with(
        &self,
        root: &WorkRoot,
        args: Vec<String>,
        env: &[(String, String)],
        stdin: Option<Vec<u8>>,
    ) -> Result<Vec<u8>, StoreFault> {
        match self.exec(root, args, env, stdin).await {
            Ok(out) if out.succeeded() => Ok(out.stdout),
            _ => Err(StoreFault::Failed),
        }
    }

    /// The folder is inside a repository's work tree; otherwise `NoHistory`.
    pub(crate) async fn require_repository(&self, root: &WorkRoot) -> Result<(), StoreFault> {
        match self
            .exec(root, strings(&["rev-parse", "--show-toplevel"]), &[], None)
            .await
        {
            Ok(out) if out.succeeded() => Ok(()),
            _ => Err(StoreFault::NoHistory),
        }
    }

    /// `GIT_INDEX_FILE` for a private index of `session`, with its folder made.
    pub(crate) async fn index_env(
        &self,
        session: &SessionId,
        kind: IndexKind,
    ) -> Result<Vec<(String, String)>, StoreFault> {
        let dir = self.index_dir.as_str().to_owned();
        let made = {
            let dir = dir.clone();
            tokio::task::spawn_blocking(move || std::fs::create_dir_all(dir)).await
        };
        if !matches!(made, Ok(Ok(()))) {
            return Err(StoreFault::Failed);
        }
        let file = format!("{dir}/{}", index_file(session, kind));
        Ok(vec![("GIT_INDEX_FILE".to_owned(), file)])
    }

    /// The tree of point `id`; `NoSuchPoint` if the session holds none by that number.
    pub(crate) async fn point_tree(
        &self,
        root: &WorkRoot,
        session: &SessionId,
        id: CheckpointId,
    ) -> Result<String, StoreFault> {
        let spec = format!("{}^{{tree}}", ref_name(session, id));
        let args = vec![
            "rev-parse".to_owned(),
            "--verify".to_owned(),
            "--quiet".to_owned(),
            spec,
        ];
        match self.exec(root, args, &[], None).await {
            Ok(out) if out.succeeded() => object_id(&out.stdout).map_err(|_| StoreFault::Failed),
            Ok(_) => Err(StoreFault::NoSuchPoint),
            Err(_) => Err(StoreFault::Failed),
        }
    }

    /// The files of a saved tree, relative to the folder.
    pub(crate) async fn listing_then(
        &self,
        root: &WorkRoot,
        tree: &str,
    ) -> Result<TreeListing, StoreFault> {
        let args = vec![
            "ls-tree".to_owned(),
            "-r".to_owned(),
            "-z".to_owned(),
            tree.to_owned(),
        ];
        let out = self.ok_with(root, args, &[], None).await?;
        tree_listing(&out).map_err(|_| StoreFault::Failed)
    }

    /// The files of the folder as they are now, read through the session's plan index. Nothing
    /// is stored.
    pub(crate) async fn listing_now(
        &self,
        root: &WorkRoot,
        session: &SessionId,
    ) -> Result<TreeListing, StoreFault> {
        let env = self.index_env(session, IndexKind::Planned).await?;
        self.ok(root, &["add", "-A", "--", "."], &env).await?;
        let out = self.ok(root, &["ls-files", "-s", "-z"], &env).await?;
        staged_listing(&out).map_err(|_| StoreFault::Failed)
    }
}

/// Owned arguments from literals.
pub(crate) fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}
