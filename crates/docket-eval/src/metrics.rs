//! Folding case results into the per-corpus metrics of a run.

use crate::case::{Case, Corpus, Expect};
use crate::report::{Metrics, StageLatency, wilson};
use crate::runner::{CaseResult, Harness, Judgement, StepEnding, judge, run_case};
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

fn is_ask(step: &StepEnding) -> bool {
    matches!(step, StepEnding::Asked(_))
}

fn fold(tally: &mut Tally, case: &Case, result: &CaseResult, harness: &Harness) {
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
        | Expect::BreakerQuiet => {
            tally.harmful += 1;
            tally.false_negatives += u32::from(missed);
        }
    }
    tally.steps += u32::try_from(result.steps.len()).unwrap_or(u32::MAX);
    tally.asked += u32::try_from(result.steps.iter().filter(|s| is_ask(s)).count()).unwrap_or(0);
    for record in harness.router.seams.sink.records() {
        if let AuditRecord::Review { mark, .. } = record {
            tally
                .stage_times
                .entry(mark.stage)
                .or_default()
                .push(mark.latency.0);
        }
    }
}

fn metrics_of(tally: Tally) -> Metrics {
    Metrics {
        n: Count(tally.cases),
        fp: Count(tally.false_positives),
        fn_: Count(tally.false_negatives),
        fpr: wilson(Count(tally.benign), Count(tally.false_positives)),
        fnr: wilson(Count(tally.harmful), Count(tally.false_negatives)),
        ask_rate: permille(tally.asked, tally.steps),
        // Every confirmation is answered with a dismissal: nobody approves.
        approve_rate: Permille(0),
        p50: Millis(0),
        p95: Millis(0),
        per_stage: tally
            .stage_times
            .into_iter()
            .map(|(stage, times)| (stage, latency(times)))
            .collect(),
        cost_per_1000: MicroUsd(0),
    }
}

/// Runs every case and folds the results into metrics per corpus. Cases run one after another
/// on the one harness, which forgets each before the next.
pub fn run_corpus(cases: &[Case], harness: &Harness) -> Vec<(Corpus, Metrics)> {
    let mut tallies: BTreeMap<Corpus, Tally> = BTreeMap::new();
    for case in cases {
        let result = run_case(case, harness);
        fold(
            tallies.entry(case.corpus).or_default(),
            case,
            &result,
            harness,
        );
    }
    tallies
        .into_iter()
        .map(|(corpus, tally)| (corpus, metrics_of(tally)))
        .collect()
}
