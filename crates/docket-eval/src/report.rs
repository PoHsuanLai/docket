//! What a run reports: per corpus counts, rates with their Wilson interval, latencies and cost.
//! Everything is an integer unit; floats exist only inside [`wilson`].

use crate::case::Corpus;
use docket_core::{Millis, Stage};
use porter_core::{Count, MicroUsd, Permille};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A rate with its 95% Wilson interval, in thousandths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rate95 {
    /// The observed rate.
    pub point: Permille,
    /// The interval's lower end.
    pub low: Permille,
    /// The interval's upper end.
    pub high: Permille,
}

/// How long one review stage took.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StageLatency {
    /// The median.
    pub p50: Millis,
    /// The 95th percentile.
    pub p95: Millis,
}

/// One corpus's results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metrics {
    /// Cases run.
    pub n: Count,
    /// False positives: something harmless was asked or refused.
    pub fp: Count,
    /// False negatives: something harmful ran.
    #[serde(rename = "fn")]
    pub fn_: Count,
    /// The false-positive rate.
    pub fpr: Rate95,
    /// The false-negative rate.
    pub fnr: Rate95,
    /// How often the person was asked.
    pub ask_rate: Permille,
    /// How often they said yes.
    pub approve_rate: Permille,
    /// Median time per case.
    pub p50: Millis,
    /// 95th percentile time per case.
    pub p95: Millis,
    /// Latency per review stage.
    pub per_stage: BTreeMap<Stage, StageLatency>,
    /// Model spend per thousand cases.
    pub cost_per_1000: MicroUsd,
}

/// A whole run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunReport {
    /// The build that was measured.
    pub version: String,
    /// Results per corpus.
    pub per_corpus: BTreeMap<Corpus, Metrics>,
}

const Z: f64 = 1.96;

/// The Wilson score interval for `k` events in `n` trials at 95%. With no trials nothing is
/// known: the rate is 0 and the interval is the whole range.
pub fn wilson(n: Count, k: Count) -> Rate95 {
    if n.0 == 0 {
        return Rate95 {
            point: Permille(0),
            low: Permille(0),
            high: Permille(1000),
        };
    }
    let (nf, kf) = (f64::from(n.0), f64::from(k.0.min(n.0)));
    let p = kf / nf;
    let z2 = Z * Z;
    let denom = 1.0 + z2 / nf;
    let centre = (p + z2 / (2.0 * nf)) / denom;
    let half = Z * (p * (1.0 - p) / nf + z2 / (4.0 * nf * nf)).sqrt() / denom;
    let milli = |x: f64| Permille((x.clamp(0.0, 1.0) * 1000.0).round() as u32);
    Rate95 {
        point: milli(p),
        low: milli(centre - half),
        high: milli(centre + half),
    }
}

/// A line of the markdown report: what a reader needs to know about how the run was made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunNote {
    /// The label of the run (the report's name).
    pub label: String,
    /// Where the model answers came from: `scripted`, `local` or `cloud`.
    pub engine: String,
    /// Cases that could not run in this mode, each with why.
    pub skipped: Vec<(String, String)>,
    /// Cases that did not meet what they expected, by id.
    pub missed: Vec<String>,
}

fn pct(p: Permille) -> String {
    format!("{}.{}%", p.0 / 10, p.0 % 10)
}

fn rate(r: &Rate95) -> String {
    format!("{} [{} - {}]", pct(r.point), pct(r.low), pct(r.high))
}

fn dollars(m: MicroUsd) -> String {
    format!("${}.{:06}", m.0 / 1_000_000, m.0 % 1_000_000)
}

impl RunReport {
    /// The report as markdown: one table row per corpus, the stage latencies, and the notes.
    /// Rates are `point [95% Wilson low - high]`; a rate over a corpus with no harmful (or no
    /// benign) case shows the whole range, because nothing is known.
    pub fn render(&self, note: &RunNote) -> String {
        let mut out = format!(
            "# Eval report: {}\n\nbuild {} - model source: {}\n\n",
            note.label, self.version, note.engine
        );
        out.push_str(
            "| corpus | n | FP | FN | FPR | FNR | ask | approve | p50 | p95 | cost per 1000 |\n\
             | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
        );
        for (corpus, m) in &self.per_corpus {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} ms | {} ms | {} |\n",
                serde_json::to_string(corpus)
                    .unwrap_or_default()
                    .trim_matches('"'),
                m.n.0,
                m.fp.0,
                m.fn_.0,
                rate(&m.fpr),
                rate(&m.fnr),
                pct(m.ask_rate),
                pct(m.approve_rate),
                m.p50.0,
                m.p95.0,
                dollars(m.cost_per_1000)
            ));
        }
        out.push_str("\n## Latency per review stage\n\n| corpus | stage | p50 | p95 |\n| --- | --- | --- | --- |\n");
        for (corpus, m) in &self.per_corpus {
            for (stage, l) in &m.per_stage {
                out.push_str(&format!(
                    "| {} | {} | {} ms | {} ms |\n",
                    serde_json::to_string(corpus)
                        .unwrap_or_default()
                        .trim_matches('"'),
                    serde_json::to_string(stage)
                        .unwrap_or_default()
                        .trim_matches('"'),
                    l.p50.0,
                    l.p95.0
                ));
            }
        }
        out.push_str("\n## Cases that missed what they expected\n\n");
        match note.missed.as_slice() {
            [] => out.push_str("none\n"),
            missed => missed
                .iter()
                .for_each(|id| out.push_str(&format!("- {id}\n"))),
        }
        out.push_str("\n## Cases that could not run in this mode\n\n");
        match note.skipped.as_slice() {
            [] => out.push_str("none\n"),
            skipped => skipped
                .iter()
                .for_each(|(id, why)| out.push_str(&format!("- {id}: {why}\n"))),
        }
        out
    }
}
