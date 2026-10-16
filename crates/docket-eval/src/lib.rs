//! The red-team and eval suite that ships with the gate (addendum A9). It holds the corpus
//! format and loader, the case and report types, the pure judgement of a finished case, the
//! Wilson interval, and the runner skeleton over `docket-fake`.
//!
//! `cargo test -p docket-eval` runs every corpus except `UiSpoofing` through the deterministic
//! layers; the release run with real models is `scripts/eval-release.sh`.

mod attack;
mod attack_live;
mod attack_live_report;
mod attack_place;
mod attack_score;
mod attack_summary;
mod block;
mod case;
mod cassette;
mod check;
mod corpus;
mod metrics;
mod model_script;
mod planner_case;
mod report;
mod runner;
mod shadow;
mod shadow_render;
mod skills;
mod steps;
mod trace;
mod ui;
mod world;

pub use attack::{
    AttackCase, AttackError, AttackGoal, AttackStyle, Carrier, PLACEHOLDER, load_attacks,
};
pub use attack_live::{
    CarrierKind, LiveRun, LiveScore, faulted_trace, live_trace, planner_slug, score_live,
};
pub use attack_live_report::{LiveMetrics, ThreeRates};
pub use attack_place::{Place, PlaceError, PlaceSpec};
pub use attack_score::{
    AttackMetrics, AttackScore, GoalOutcome, StyleRates, Utility, run_attacks, score_endings,
    score_traces,
};
pub use attack_summary::{
    AttackSummary, Comparison, FAKE_ENGINE, Significance, SummaryError, Verdict, short_commit,
    today, utc_date,
};
pub use case::{
    ArgFrom, Case, CaseId, ConsentFixture, Corpus, Driver, Expect, FixtureContact, FixtureFile,
    FixtureMail, FixtureRef, FixtureTask, MailField, ScriptedCall, ScriptedSend, ScriptedStep,
    WorldFixture,
};
pub use cassette::cassette_from;
pub use check::{CheckError, CheckReport, Finding, Level, check_app};
pub use corpus::{CorpusError, load_all, load_corpus};
pub use metrics::{Observed, Tallies, run_corpus};
pub use model_script::ModelScript;
pub use planner_case::{
    PlannerCase, PlannerCaseError, PlannerEnd, PlannerExpect, SheetAnswer, load_planner_cases,
};
pub use report::{Metrics, Rate95, RunNote, RunReport, StageLatency, wilson};
pub use runner::{
    CaseResult, Harness, Judgement, PolicyMode, Rig, StepEnding, judge, maximal_policy, run_case,
    run_case_traced,
};
pub use shadow::{
    Label, ShadowCase, ShadowReport, THRESHOLDS, auc, false_negatives, false_positives,
};
pub use skills::check_skills;
pub use trace::{
    CaseTrace, StepTrace, audit_line, plan_line, render_exchange, render_index, trace_file,
};
pub use ui::{UiCommand, UiFault, UiFile, UiOnly, UiSource, check_ui};
pub use world::install_fixture;
