//! `GitStore`: the commands it runs and the listings it reads. The trait impl is in `store.rs`.

use crate::files::{inside_repository, person_excludes};
use crate::names::{IndexKind, index_file, ref_name};
use crate::parse::{object_id, staged_listing, tree_listing};
use crate::problem::{GitCause, GitProblem, Report};
use crate::run::{GitCommand, GitFault, GitOut, GitRun, Limits};
use docket_checkpoint::{StoreFault, TreeListing, WorkRoot};
use docket_core::{AbsPath, CheckpointId};
use prov::SessionId;
use std::path::{Path, PathBuf};

/// Options before every command, none of them from the folder: no background refresh, no pager,
/// no hooks, no file-system monitor, no attributes file, no signing, no auto-maintenance, no
/// network protocol. They are the second line behind the shadow repository (see the crate docs).
const FIXED: [&str; 16] = [
    "--no-optional-locks",
    "--no-pager",
    "-c",
    "core.bare=false",
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "core.attributesFile=/dev/null",
    "-c",
    "commit.gpgsign=false",
    "-c",
    "gc.auto=0",
    "-c",
    "protocol.allow=never",
];

/// Variables set after everything else, so no caller can undo them: no system or user git
/// configuration, no system attributes, no prompt.
const HARDENING: [(&str, &str); 5] = [
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_CONFIG_SYSTEM", "/dev/null"),
    ("GIT_ATTR_NOSYSTEM", "1"),
    ("GIT_TERMINAL_PROMPT", "0"),
];

/// Restore points of a folder that is in a git repository, kept in a repository of docket's own.
///
/// Every point is a commit with no parent, made from a private index (a file under `index_dir`,
/// one set per session), kept under `refs/docket/checkpoints/<session>/<n>` in `shadow.git`
/// under `index_dir`. The folder's own `.git` is never read by git: its config, hooks and
/// attributes belong to whoever can write the folder. The person's own index, HEAD, branches and
/// stash are never named; only `apply` writes files of the folder.
#[derive(Debug)]
pub struct GitStore<R: GitRun> {
    pub(crate) run: R,
    index_dir: AbsPath,
    env: Vec<(String, String)>,
    limits: Limits,
    report: Report,
}

impl<R: GitRun> GitStore<R> {
    /// A store that runs git through `run` and keeps its private indexes in `index_dir`, a folder
    /// of docket's own (never inside a repository's `.git`).
    pub fn new(run: R, index_dir: AbsPath) -> Self {
        Self {
            run,
            index_dir,
            env: Vec::new(),
            limits: Limits::default(),
            report: Report::default(),
        }
    }

    /// The deadline and the output limit of every command (the defaults are generous).
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Where the cause of a failed command goes (the operator's log): the subcommand, how it
    /// ended and the tail of its standard error. Never put it in front of a person.
    pub fn with_problems(mut self, sink: impl Fn(&GitProblem) + Send + Sync + 'static) -> Self {
        self.report = Report::to(sink);
        self
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
        let mut all = self.before(root);
        all.extend(args);
        let mut vars = self.env.clone();
        vars.extend(env.iter().cloned());
        vars.extend(
            HARDENING
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
        );
        self.run
            .run(GitCommand {
                cwd: root.path().clone(),
                args: all,
                env: vars,
                stdin,
                limits: self.limits,
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
        let command = args.first().cloned().unwrap_or_default();
        match self.exec(root, args, env, stdin).await {
            Ok(out) if out.succeeded() => Ok(out.stdout),
            Ok(out) => {
                let (code, stderr) = (out.status.code(), lossy(&out.stderr));
                self.tell(command, GitCause::Exit { code, stderr });
                Err(StoreFault::Failed)
            }
            // Too many files to list is the folder being too large, not a git failure.
            Err(GitFault::TooMuchOutput) => {
                self.tell(command, GitCause::TooMuchOutput);
                Err(StoreFault::TooLarge)
            }
            Err(fault) => {
                self.tell(command, fault.into());
                Err(StoreFault::Failed)
            }
        }
    }

    /// Tells the sink why `command` failed.
    fn tell(&self, command: String, cause: GitCause) {
        self.report.tell(&GitProblem { command, cause });
    }

    /// The repository docket keeps every point in: under `index_dir`, never the folder's `.git`.
    fn shadow(&self) -> String {
        format!("{}/shadow.git", self.index_dir.as_str())
    }

    /// The options before every command: the shadow repository as `--git-dir`, the folder as
    /// `--work-tree`, then [`FIXED`]. The person's `.git/info/exclude` is the one file of the
    /// folder's repository that is read, and only as a list of patterns.
    fn before(&self, root: &WorkRoot) -> Vec<String> {
        let folder = root.path().as_str();
        let excludes = person_excludes(Path::new(folder)).unwrap_or_else(|| "/dev/null".to_owned());
        let mut all = vec![
            format!("--git-dir={}", self.shadow()),
            format!("--work-tree={folder}"),
        ];
        all.extend(FIXED.iter().map(|part| (*part).to_owned()));
        all.push("-c".to_owned());
        all.push(format!("core.excludesFile={excludes}"));
        all
    }

    /// The folder is inside a repository's work tree (a `.git` in it or above it, looked for
    /// without running git); otherwise `NoHistory`. Then the shadow repository exists.
    pub(crate) async fn require_repository(&self, root: &WorkRoot) -> Result<(), StoreFault> {
        let folder = PathBuf::from(root.path().as_str());
        let inside = tokio::task::spawn_blocking(move || inside_repository(&folder))
            .await
            .map_err(|_| StoreFault::Failed)?;
        if !inside {
            return Err(StoreFault::NoHistory);
        }
        self.ensure_shadow().await
    }

    /// Makes `shadow.git` (bare, no templates, so no hooks) if it is not there yet.
    async fn ensure_shadow(&self) -> Result<(), StoreFault> {
        let (dir, shadow) = (self.index_dir.as_str().to_owned(), self.shadow());
        let ready = {
            let shadow = shadow.clone();
            tokio::task::spawn_blocking(move || {
                std::fs::create_dir_all(&dir)?;
                Ok::<bool, std::io::Error>(Path::new(&shadow).join("HEAD").is_file())
            })
            .await
        };
        match ready {
            Ok(Ok(true)) => return Ok(()),
            Ok(Ok(false)) => {}
            _ => return Err(StoreFault::Failed),
        }
        let args = strings(&["init", "--bare", "--quiet", "--template=", &shadow]);
        let mut env = self.env.clone();
        env.extend(
            HARDENING
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
        );
        let made = self
            .run
            .run(GitCommand {
                cwd: self.index_dir.clone(),
                args,
                env,
                stdin: None,
                limits: self.limits,
            })
            .await;
        match made {
            Ok(out) if out.succeeded() => Ok(()),
            Ok(out) => {
                let (code, stderr) = (out.status.code(), lossy(&out.stderr));
                self.tell("init".to_owned(), GitCause::Exit { code, stderr });
                Err(StoreFault::Failed)
            }
            Err(fault) => {
                self.tell("init".to_owned(), fault.into());
                Err(StoreFault::Failed)
            }
        }
    }

    /// Removes from the private index (`env`) the files the folder ignores now. An index keeps
    /// what it was given, so a file that became ignored after it was saved would stay in every
    /// later point, count against the size limit, and show as a file a restore deletes.
    pub(crate) async fn drop_ignored(
        &self,
        root: &WorkRoot,
        env: &[(String, String)],
    ) -> Result<(), StoreFault> {
        let ignored = self
            .ok(
                root,
                &["ls-files", "-c", "-i", "--exclude-standard", "-z"],
                env,
            )
            .await?;
        if ignored.is_empty() {
            return Ok(());
        }
        let args = strings(&["update-index", "--force-remove", "-z", "--stdin"]);
        self.ok_with(root, args, env, Some(ignored)).await?;
        Ok(())
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
            Err(fault) => {
                self.tell("rev-parse".to_owned(), fault.into());
                Err(StoreFault::Failed)
            }
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
        self.drop_ignored(root, &env).await?;
        self.ok(root, &["add", "-A", "--", "."], &env).await?;
        let out = self.ok(root, &["ls-files", "-s", "-z"], &env).await?;
        staged_listing(&out).map_err(|_| StoreFault::Failed)
    }
}

/// Standard error as text, whatever bytes it holds.
fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Owned arguments from literals.
pub(crate) fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}
