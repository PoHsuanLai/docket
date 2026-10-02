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
