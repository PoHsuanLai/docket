//! The red-team suite as values (addendum A9): the case and corpus format, the attack format,
//! how a finished case ended and the pure judgement of it, the Wilson interval, the per-corpus
//! metrics, the attack scores and the per-commit summary, and the run trace and its rendering.
//!
//! Nothing here reads a file, the clock or the environment, and nothing here links the router,
//! Cedar or the fakes: `docket-eval` reads the corpus and plays the cases over `docket-fake`,
//! then hands the endings to this crate. The facts about actions a score needs (the effect of a
//! call, the label of a sheet) come in as closures.

mod attack;
mod attack_live;
mod attack_live_report;
mod attack_needs;
mod attack_place;
mod attack_score;
mod attack_summary;
mod case;
mod metrics;
mod model_script;
mod outcome;
mod report;
mod trace;

pub use attack::{AttackCase, AttackError, AttackGoal, AttackStyle, Carrier, PLACEHOLDER};
pub use attack_live::{
    CarrierKind, LiveRun, LiveScore, faulted_trace, live_trace, planner_slug, score_live,
};
pub use attack_live_report::{LiveMetrics, ThreeRates};
pub use attack_needs::{AnswerGiven, UserNeeds};
pub use attack_place::{Place, PlaceError, PlaceSpec};
pub use attack_score::{
    AttackMetrics, AttackScore, GoalOutcome, StyleRates, Utility, score_endings, score_traces,
};
pub use attack_summary::{
    AttackSummary, Comparison, FAKE_ENGINE, Significance, SummaryError, Verdict, utc_date,
};
pub use case::{
    ArgFrom, Case, CaseId, CasePolicy, ConsentFixture, Corpus, Driver, Expect, FixtureContact,
    FixtureFile, FixtureMail, FixtureRef, FixtureTask, MailField, ScriptedCall, ScriptedSend,
    ScriptedStep, WorldFixture,
};
pub use metrics::{Observed, Tallies};
pub use model_script::ModelScript;
pub use outcome::{CaseResult, Judgement, StepEnding, judge, reached};
pub use report::{Metrics, Rate95, RunNote, RunReport, StageLatency, wilson};
pub use trace::{
    CaseTrace, Cut, StepTrace, audit_line, plan_line, render_exchange, render_index, trace_file,
};
