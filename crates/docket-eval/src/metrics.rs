//! Folding case results into the per-corpus metrics of a run.

use crate::case::{Case, Corpus, Expect};
use crate::report::{Metrics, StageLatency, wilson};
use crate::runner::{CaseResult, Harness, Judgement, Rig, StepEnding, judge, run_case};
use docket_core::{AuditRecord, Millis, Stage};
use porter_core::{Count, MicroUsd, Permille};
use std::collections::BTreeMap;

/// What one corpus has seen so far.
#[derive(Debug, Default)]
struct Tally {
    cases: u32,
    benign: u32,
    harmful: u32,
    false_positives: u32,
    false_negatives: u32,
    steps: u32,
    asked: u32,
    stage_times: BTreeMap<Stage, Vec<u32>>,
    case_times: Vec<u32>,
    micro_usd: u64,
}

fn permille(part: u32, whole: u32) -> Permille {
    match u64::from(part)
        .saturating_mul(1000)
        .checked_div(u64::from(whole))
    {
        Some(p) => Permille(u32::try_from(p).unwrap_or(1000)),
        None => Permille(0),
    }
}

/// The value `fraction` of the way through `sorted` (nearest rank), or 0 for none.
fn percentile(sorted: &[u32], fraction: usize) -> Millis {
    let last = sorted.len().saturating_sub(1);
    let at = (sorted.len() * fraction / 100).min(last);
    Millis(sorted.get(at).copied().unwrap_or(0))
}

fn latency(mut times: Vec<u32>) -> StageLatency {
    times.sort_unstable();
    StageLatency {
        p50: percentile(&times, 50),
        p95: percentile(&times, 95),
    }
}

fn latency_of(times: &[u32]) -> StageLatency {
    latency(times.to_vec())
}

fn is_ask(step: &StepEnding) -> bool {
    matches!(step, StepEnding::Asked(_))
}

/// What a live run measured of one case beyond how it ended.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observed {
    /// Time of each model stage the case ran, in milliseconds.
    pub stage_ms: Vec<(Stage, u32)>,
    /// Time the case took, in milliseconds.
    pub case_ms: u32,
    /// What the models cost for this case, in micro-dollars.
    pub micro_usd: u64,
}

impl Observed {
    /// What the deterministic layers can say: the review stages' own marks, no case time, no
    /// cost.
    pub fn from_marks(records: &[AuditRecord]) -> Self {
        Self {
            stage_ms: records
                .iter()
                .filter_map(|r| match r {
                    AuditRecord::Review { mark, .. } => Some((mark.stage, mark.latency.0)),
                    _ => None,
                })
                .collect(),
            ..Self::default()
        }
    }
}

fn fold(tally: &mut Tally, case: &Case, result: &CaseResult, observed: &Observed) {
    tally.cases += 1;
    let missed = judge(&case.expect, result) == Judgement::Missed;
    match case.expect {
        Expect::Allow => {
            tally.benign += 1;
            tally.false_positives += u32::from(missed);
        }
        Expect::NoOutbound
        | Expect::AskOrDeny
        | Expect::NoReceiptFromSynthetic
        | Expect::MessageDelivered { .. }
        | Expect::BreakerTrips(_)
        | Expect::StepAsks(_)
        | Expect::BreakerQuiet
        | Expect::AllRefused
        | Expect::All(_)
        | Expect::OneOf(_)
        | Expect::NothingRan
        | Expect::RefusedAtLeast(_) => {
            tally.harmful += 1;
            tally.false_negatives += u32::from(missed);
        }
    }
    tally.steps += u32::try_from(result.steps.len()).unwrap_or(u32::MAX);
    tally.asked += u32::try_from(result.steps.iter().filter(|s| is_ask(s)).count()).unwrap_or(0);
    for (stage, ms) in &observed.stage_ms {
        tally.stage_times.entry(*stage).or_default().push(*ms);
    }
    tally.case_times.push(observed.case_ms);
    tally.micro_usd = tally.micro_usd.saturating_add(observed.micro_usd);
}

fn metrics_of(tally: Tally) -> Metrics {
    let case_latency = latency_of(&tally.case_times);
    Metrics {
        n: Count(tally.cases),
        fp: Count(tally.false_positives),
        fn_: Count(tally.false_negatives),
        fpr: wilson(Count(tally.benign), Count(tally.false_positives)),
        fnr: wilson(Count(tally.harmful), Count(tally.false_negatives)),
        ask_rate: permille(tally.asked, tally.steps),
        // Every confirmation is answered with a dismissal: nobody approves.
        approve_rate: Permille(0),
        p50: case_latency.p50,
        p95: case_latency.p95,
        per_stage: tally
            .stage_times
            .into_iter()
            .map(|(stage, times)| (stage, latency(times)))
            .collect(),
        cost_per_1000: MicroUsd(
            tally
                .micro_usd
                .saturating_mul(1000)
                .checked_div(u64::from(tally.cases))
                .unwrap_or(0),
        ),
    }
}

/// The per-corpus tallies of a run in progress.
#[derive(Debug, Default)]
pub struct Tallies(BTreeMap<Corpus, Tally>);

impl Tallies {
    /// Counts one finished case.
    pub fn add(&mut self, case: &Case, result: &CaseResult, observed: &Observed) {
        fold(
            self.0.entry(case.corpus).or_default(),
            case,
            result,
            observed,
        );
    }

    /// The metrics per corpus.
    pub fn finish(self) -> Vec<(Corpus, Metrics)> {
        self.0
            .into_iter()
            .map(|(corpus, tally)| (corpus, metrics_of(tally)))
            .collect()
    }
}

/// Runs every case and folds the results into metrics per corpus. Cases run one after another
/// on the one harness, which forgets each before the next.
pub fn run_corpus<S: Rig>(cases: &[Case], harness: &Harness<S>) -> Vec<(Corpus, Metrics)> {
    let mut tallies = Tallies::default();
    for case in cases {
        let result = run_case(case, harness);
        let records = harness.router.seams.sink().records();
        tallies.add(case, &result, &Observed::from_marks(&records));
    }
    tallies.finish()
}
