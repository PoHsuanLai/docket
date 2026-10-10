//! The git store of restore points.
//!
//! A restore point is a hidden commit of the workspace with no parent, made from a private index
//! (a file docket owns, never the person's index) and kept under
//! `refs/docket/checkpoints/<session>/<n>`, a ref family no branch or tag listing shows. Taking a
//! point writes only objects, that private index and the new ref. Applying one is the only thing
//! that writes the workspace's files, after re-deriving the plan and checking its digest.
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
