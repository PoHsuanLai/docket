//! The red-team and eval suite that ships with the gate (addendum A9): the reading of the
//! corpus and the attack files, the conformance checks, and the runner over `docket-fake`. The
//! case and report types, the judgement of a finished case, the Wilson interval, the scores and
//! the summaries are `eval-core`'s.
//!
//! `cargo test -p docket-eval` runs every corpus except `UiSpoofing` through the deterministic
//! layers; the release run with real models is `scripts/eval-release.sh`.

mod attack_load;
mod block;
mod cassette;
mod check;
mod corpus;
mod facts;
mod planner_case;
mod runner;
mod skills;
mod steps;
mod summary_io;
mod sweep;
mod ui;
mod world;

pub use attack_load::load_attacks;
pub use cassette::cassette_from;
pub use check::{CheckError, CheckReport, Finding, Level, check_app};
pub use corpus::{CorpusError, load_all, load_corpus};
pub use facts::Facts;
pub use planner_case::{
    PlannerCase, PlannerCaseError, PlannerEnd, PlannerExpect, SheetAnswer, load_planner_cases,
};
pub use runner::{Harness, PolicyMode, Rig, maximal_policy, run_case, run_case_traced};
pub use skills::check_skills;
pub use summary_io::{read_summary, short_commit, stamp_summary, write_summary};
pub use sweep::{run_attacks, run_corpus};
pub use ui::{UiCommand, UiFault, UiFile, UiOnly, UiSource, check_ui};
pub use world::{SetupFault, install_fixture};
