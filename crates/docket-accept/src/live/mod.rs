//! The live-eval harness: runs the agent stack against a real model (or a cassette) with every
//! safety the owner's machine needs: a private bus, scratch HOME and XDG directories, no real
//! mail or memory, and the network only when the engine is `cloud`. See `docs/live-eval.md`.

pub mod cli;
pub mod corpus;
pub mod engine;
pub mod flows;
pub mod inferd_world;
pub mod regress;
pub mod stage;
pub mod trace_dir;

pub use corpus::{CorpusError, CorpusOptions, CorpusOutcome, Timeouts, run_corpus_live};
pub use engine::{Engine, EngineError, Reach, hijacked_judge_cassette};
pub use inferd_world::InferdWorld;

use crate::world::Binaries;
use std::path::PathBuf;

/// The binaries the packaged `docket-live` runs: the daemons beside it, and the two siblings
/// `build.rs` built into the target directory.
pub fn packaged_binaries() -> std::io::Result<Binaries> {
    let exe = std::env::current_exe()?;
    let beside = |name: &str| exe.with_file_name(name);
    Ok(Binaries {
        intentd: beside("accept-intentd"),
        companiond: beside("accept-companiond"),
        readerd: beside("accept-readerd"),
        memoryd: PathBuf::from(env!("ACCEPT_MEMORYD")),
        inferd: PathBuf::from(env!("ACCEPT_INFERD")),
        quire_do: beside("accept-quire-do"),
    })
}
