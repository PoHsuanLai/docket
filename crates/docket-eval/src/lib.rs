//! The red-team and eval suite that ships with the gate (addendum A9). It holds the corpus
//! format and loader, the case and report types, the pure judgement of a finished case, the
//! Wilson interval, and the runner skeleton over `docket-fake`.
//!
//! `cargo test -p docket-eval` runs every corpus except `UiSpoofing` through the deterministic
//! layers; the release run with real models is `scripts/eval-release.sh`.

mod block;
mod case;
mod check;
mod corpus;
mod metrics;
mod report;
mod runner;
mod skills;
mod steps;
mod ui;
mod world;

pub use case::{
    ArgFrom, Case, CaseId, ConsentFixture, Corpus, Driver, Expect, FixtureContact, FixtureFile,
    FixtureMail, FixtureRef, FixtureTask, MailField, ScriptedCall, ScriptedSend, ScriptedStep,
    WorldFixture,
};
pub use check::{CheckError, CheckReport, Finding, Level, check_app};
pub use corpus::{CorpusError, load_all, load_corpus};
pub use metrics::run_corpus;
pub use report::{Metrics, Rate95, RunReport, StageLatency, wilson};
pub use runner::{CaseResult, Harness, Judgement, StepEnding, judge, maximal_policy, run_case};
pub use skills::check_skills;
pub use ui::{UiCommand, UiFile, UiOnly, UiSource, check_ui};
