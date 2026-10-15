//! The git store of restore points.
//!
//! A restore point is a hidden commit of the workspace with no parent, made from a private index
//! (a file docket owns, never the person's index) and kept under
//! `refs/docket/checkpoints/<session>/<n>`, a ref family no branch or tag listing shows. Taking a
//! point writes only objects, that private index and the new ref. Applying one is the only thing
//! that writes the workspace's files, after re-deriving the plan and checking its digest.
//!
//! # Threat: the folder is the agent's
//!
//! The agent can write the whole work tree, its `.git` included: `config`, `hooks/`,
//! `info/attributes` and every `.gitattributes`. git run against such a repository executes what
//! those name (hooks, `filter.<x>.clean`, `core.fsmonitor`, `core.sshCommand`, `include.path`),
//! and docket runs as the person, outside the agent's sandbox. So no git command here ever reads
//! the folder's repository. Points live in a repository of docket's own (`shadow.git` under the
//! store's private folder, which the sandbox does not bind); every command names it with
//! `--git-dir` and the folder with `--work-tree`. Its config is the one docket wrote, so it
//! defines no filter driver and no hook (a `.gitattributes` in the tree can name a driver, but
//! nothing defines it). On top of that every command refuses hooks, attributes files, file-system
//! monitors, pagers and network protocols, reads no system or user configuration, and drops the
//! variables that redirect git. The folder is still read as data: `.gitignore` files and the
//! top-level `.git/info/exclude` (a regular file, no link on the way) as patterns, and the
//! files themselves. Whether a folder is "in a repository" is decided by looking for a `.git`
//! entry on the way up, without running git.
//!
//! git runs as a process behind [`GitRun`] (no shell, a list of arguments, no inherited
//! `GIT_DIR` or `GIT_INDEX_FILE`); [`StdGitRun`] is the real one. [`GitStore`] is the
//! `CheckpointStore`.

mod files;
mod git;
mod names;
mod parse;
mod run;
mod store;

pub use git::GitStore;
pub use names::ref_name;
pub use run::{GitCommand, GitFault, GitOut, GitRun, StdGitRun};
