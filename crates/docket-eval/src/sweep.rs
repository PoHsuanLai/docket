//! Running many cases on one harness: a corpus into metrics, the attacks into scores.

use crate::runner::{Harness, Rig, run_case};
use eval_core::{
    AttackCase, AttackError, AttackScore, Case, Corpus, Metrics, Observed, Tallies, score_endings,
};

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

/// Plays every attack on `harness`, both cases of each, and scores them. The harness decides who
/// answers the reviewers: the default one allows everything (a hijacked judge).
pub fn run_attacks<S: Rig>(
    attacks: &[AttackCase],
    harness: &Harness<S>,
) -> Result<Vec<AttackScore>, AttackError> {
    attacks
        .iter()
        .map(|attack| {
            let (clean, attacked) = attack.cases();
            let clean_run = run_case(&clean, harness);
            let attacked_run = run_case(&attacked, harness);
            score_endings(attack, &clean_run.steps, &attacked_run.steps)
        })
        .collect()
}
