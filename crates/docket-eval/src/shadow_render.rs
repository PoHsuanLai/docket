//! The shadow section of the markdown report. Appended after the existing sections, which it
//! leaves as they were.

use crate::case::Corpus;
use crate::report::Rate95;
use crate::shadow::{
    Label, ShadowReport, THRESHOLDS, auc, false_negatives, false_positives, flagged_at,
    flagged_where_live_passed,
};
use action_review::LiveCall;
use porter_core::Permille;

fn pct(p: Permille) -> String {
    format!("{}.{}%", p.0 / 10, p.0 % 10)
}

fn rate(r: &Rate95) -> String {
    format!("{} [{} - {}]", pct(r.point), pct(r.low), pct(r.high))
}

fn slug<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .unwrap_or_default()
        .trim_matches('"')
        .to_owned()
}

fn live_word(call: LiveCall) -> &'static str {
    match call {
        LiveCall::Passed => "pass",
        LiveCall::Flagged => "flag",
        LiveCall::Failed => "failed",
    }
}

const CATEGORIES: [Corpus; 4] = [
    Corpus::Injection,
    Corpus::Overeager,
    Corpus::Exfiltration,
    Corpus::AdaptiveJudge,
];

impl ShadowReport {
    /// The section: per-case P(flag), then per category AUC, false-negative rate with its
    /// Wilson interval and the flags the live Quick judge passed, then the benign false
    /// positives, each at a few thresholds. Empty when the shadow scored nothing.
    pub fn render(&self) -> String {
        if self.cases.is_empty() {
            return format!(
                "\n## Shadow flagger\n\nnot run, or scored no case ({} without a score)\n",
                self.unscored
            );
        }
        let mut out = format!(
            "\n## Shadow flagger (recorded only; it changes no verdict)\n\n{} cases scored, {} without a score. P(flag) is the highest over a case's quick reviews; AUC is the category's harmful cases against all benign ones.\n\n",
            self.cases.len(),
            self.unscored
        );
        out.push_str("### P(flag) per case\n\n| corpus | case | label | live quick | P(flag) |\n| --- | --- | --- | --- | --- |\n");
        for c in &self.cases {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                slug(&c.corpus),
                c.id.0,
                slug(&c.label),
                live_word(c.live),
                pct(c.odds)
            ));
        }
        let benign = self.of(Label::Benign, None);
        out.push_str("\n### Harmful cases per category\n\n| corpus | scored | AUC | threshold | missed | FNR [95% Wilson] | flags where live passed |\n| --- | --- | --- | --- | --- | --- | --- |\n");
        for corpus in CATEGORIES {
            let harmful = self.of(Label::Harmful, Some(corpus));
            let area = auc(&harmful, &benign).map_or("n/a".to_owned(), pct);
            for t in THRESHOLDS {
                let (missed, fnr) = false_negatives(&harmful, t);
                let in_corpus: Vec<_> = self.cases.iter().filter(|c| c.corpus == corpus).collect();
                out.push_str(&format!(
                    "| {} | {} | {} | {} | {} | {} | {} |\n",
                    slug(&corpus),
                    harmful.len(),
                    area,
                    pct(t),
                    missed.0,
                    rate(&fnr),
                    flagged_where_live_passed(&in_corpus, t).0
                ));
            }
        }
        out.push_str("\n### Benign cases\n\n| threshold | scored | flagged | FPR [95% Wilson] | flags where live passed |\n| --- | --- | --- | --- | --- |\n");
        for t in THRESHOLDS {
            let (flagged, fpr) = false_positives(&benign, t);
            debug_assert_eq!(flagged, flagged_at(&benign, t));
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                pct(t),
                benign.len(),
                flagged.0,
                rate(&fpr),
                flagged_where_live_passed(&benign, t).0
            ));
        }
        out
    }
}
