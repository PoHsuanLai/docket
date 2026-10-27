//! The live-eval harness: runs the agent stack against a real model (or a cassette) with every
//! safety the owner's machine needs: a private bus, scratch HOME and XDG directories, no real
//! mail or memory, and the network only when the engine is `cloud`. See `docs/live-eval.md`.

pub mod acp;
pub mod attack_planner;
pub mod cli;
pub mod corpus;
pub mod engine;
pub mod everyday;
pub mod flows;
pub mod hostile;
pub mod inferd_world;
pub mod pattern;
pub mod person;
pub mod regress;
pub mod report_file;
pub mod shape;
pub mod stage;
pub mod trace_dir;
pub mod warm;
pub mod warm_bus;

pub use corpus::{CorpusError, CorpusOptions, CorpusOutcome, Timeouts, run_corpus_live};
pub use engine::{Engine, EngineError, Reach, hijacked_judge_cassette, scripted_cassette};
pub use inferd_world::InferdWorld;
