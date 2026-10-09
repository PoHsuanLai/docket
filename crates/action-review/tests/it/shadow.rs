//! The shadow flagger is recorded and never obeyed: it cannot change a verdict, a slow one
//! cannot delay it, and the later combined mode can only escalate.

use crate::support::{block_on, request};
use action_review::*;
use docket_core::*;
use porter_core::Permille;

/// A reviewer that answers one way every time.
struct Fixed(Result<ReviewVerdict, ReviewError>);

impl Reviewer for Fixed {
    async fn review(&self, _: Stage, _: &ReviewRequest) -> Result<ReviewVerdict, ReviewError> {
        self.0.clone()
    }
}

/// A flagger that answers one way every time.
struct Says(Result<ShadowScore, ShadowFault>);

impl ShadowFlagger for Says {
    async fn score(&self, _: &ReviewRequest) -> Result<ShadowScore, ShadowFault> {
        self.0
    }
}

/// A flagger that never answers.
struct Hangs;

impl ShadowFlagger for Hangs {
    async fn score(&self, _: &ReviewRequest) -> Result<ShadowScore, ShadowFault> {
        std::future::pending().await
    }
}

struct Weights(Vec<(&'static str, u64)>);

impl Readout for Weights {
    async fn read(&self, _: &ReviewRequest, _: &[String]) -> Result<OptionScores, ShadowFault> {
        OptionScores::from_weights(self.0.iter().map(|(o, w)| ((*o).to_owned(), *w)).collect())
    }
}

fn why() -> ReviewReason {
    ReviewReason {
        code: ReasonCode::Uncertain,
        text: ReasonText("why".into()),
    }
}

fn lives() -> Vec<Result<ReviewVerdict, ReviewError>> {
    vec![
        Ok(ReviewVerdict::Allow),
        Ok(ReviewVerdict::Ask { why: why() }),
        Ok(ReviewVerdict::Deny { why: why() }),
        Err(ReviewError::Timeout),
        Err(ReviewError::Unparseable),
    ]
}

fn shadows() -> Vec<Result<ShadowScore, ShadowFault>> {
    vec![
        Ok(ShadowScore(Permille(0))),
        Ok(ShadowScore(Permille(299))),
        Ok(ShadowScore(Permille(300))),
        Ok(ShadowScore(Permille(1000))),
        Err(ShadowFault::Unreadable),
        Err(ShadowFault::Unavailable),
        Err(ShadowFault::Off),
    ]
}

#[test]
fn a_shadow_never_changes_a_verdict() {
    for live in lives() {
        for shadow in shadows() {
            for mode in [ShadowMode::Off, ShadowMode::Record] {
                for stage in [Stage::Quick, Stage::Deliberate, Stage::SecondOpinion] {
                    let log = ShadowLog::default();
                    let reviewer = Shadowed {
                        live: Fixed(live.clone()),
                        flagger: Says(shadow),
                        sink: log.clone(),
                        mode,
                    };
                    let got = block_on(reviewer.review(stage, &request()));
                    assert_eq!(got, live, "{stage:?} {mode:?} {shadow:?}");
                }
            }
        }
    }
}

#[test]
fn only_the_quick_stage_in_record_mode_keeps_a_note() {
    let note = |stage, mode, shadow| {
        let log = ShadowLog::default();
        let reviewer = Shadowed {
            live: Fixed(Ok(ReviewVerdict::Allow)),
            flagger: Says(shadow),
            sink: log.clone(),
            mode,
        };
        let _ = block_on(reviewer.review(stage, &request()));
        log.take()
    };
    let score = Ok(ShadowScore(Permille(700)));
    assert_eq!(
        note(Stage::Quick, ShadowMode::Record, score),
        vec![ShadowNote {
            live: LiveCall::Passed,
            shadow: score
        }]
    );
    assert!(note(Stage::Quick, ShadowMode::Off, score).is_empty());
    assert!(note(Stage::Deliberate, ShadowMode::Record, score).is_empty());
    assert!(note(Stage::Quick, ShadowMode::Record, Err(ShadowFault::Off)).is_empty());
    assert_eq!(
        note(
            Stage::Quick,
            ShadowMode::Record,
            Err(ShadowFault::Unavailable)
        )
        .len(),
        1
    );
}

#[test]
fn a_shadow_that_never_answers_cannot_hold_the_verdict_back() {
    let log = ShadowLog::default();
    let reviewer = Shadowed {
        live: Fixed(Ok(ReviewVerdict::Allow)),
        flagger: Hangs,
        sink: log.clone(),
        mode: ShadowMode::Record,
    };
    assert_eq!(
        block_on(reviewer.review(Stage::Quick, &request())),
        Ok(ReviewVerdict::Allow)
    );
    assert_eq!(
        log.take(),
        vec![ShadowNote {
            live: LiveCall::Passed,
            shadow: Err(ShadowFault::Late)
        }]
    );
}

fn looseness(v: &Result<ReviewVerdict, ReviewError>) -> u8 {
    match v {
        Ok(ReviewVerdict::Allow) => 0,
        Ok(ReviewVerdict::Ask { .. }) => 1,
        Ok(ReviewVerdict::Deny { .. }) => 2,
        Err(_) => 1,
    }
}

#[test]
fn combined_mode_can_only_escalate() {
    for live in lives() {
        assert_eq!(either_flags(live.clone(), ShadowLean::WouldPass), live);
        let flagged = either_flags(live.clone(), ShadowLean::WouldFlag);
        assert!(looseness(&flagged) >= looseness(&live), "{live:?}");
        if live != Ok(ReviewVerdict::Allow) {
            assert_eq!(flagged, live);
        }
    }
    assert!(matches!(
        either_flags(Ok(ReviewVerdict::Allow), ShadowLean::WouldFlag),
        Ok(ReviewVerdict::Ask { .. })
    ));
}

#[test]
fn a_score_leans_to_a_flag_from_the_threshold_up() {
    let at = |p| ShadowScore(Permille(p)).lean(DEFAULT_THRESHOLD);
    assert_eq!(at(299), ShadowLean::WouldPass);
    assert_eq!(at(300), ShadowLean::WouldFlag);
}

#[test]
fn the_option_flagger_reads_the_flag_share() {
    let flagger = OptionFlagger(Weights(vec![("pass", 3), ("flag", 1)]));
    assert_eq!(
        block_on(flagger.score(&request())),
        Ok(ShadowScore(Permille(250)))
    );
    let none = OptionFlagger(Weights(vec![("pass", 1)]));
    assert_eq!(
        block_on(none.score(&request())),
        Err(ShadowFault::Unreadable)
    );
    let empty = OptionFlagger(Weights(vec![("pass", 0), ("flag", 0)]));
    assert_eq!(
        block_on(empty.score(&request())),
        Err(ShadowFault::Unreadable)
    );
}

#[test]
fn scores_always_sum_to_a_thousand() {
    let scores =
        OptionScores::from_weights(vec![("a".into(), 1), ("b".into(), 1), ("c".into(), 1)])
            .expect("weights");
    let sum: u32 = ["a", "b", "c"]
        .iter()
        .filter_map(|o| scores.of(o))
        .map(|p| p.0)
        .sum();
    assert_eq!(sum, 1000);
}

#[test]
fn the_disabled_arm_scores_nothing() {
    assert_eq!(block_on(Disabled.score(&request())), Err(ShadowFault::Off));
}
