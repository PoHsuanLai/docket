//! The reviewer can only tighten. `tighten` is total, and nothing a reviewer says moves a
//! ruling from Ask or Deny to Run.

use action_review::*;
use docket_core::*;
use porter_core::{AccountId, ModelId};

fn rulings() -> Vec<(&'static str, Ruling)> {
    vec![
        ("deny", Ruling::Deny(vec![PolicyId("p".into())])),
        ("ask", Ruling::Ask(vec![AskReason::Tainted])),
        ("allow judged", Ruling::AllowJudged(vec![])),
        ("allow final", Ruling::AllowFinal(vec![])),
    ]
}

fn reason() -> ReviewReason {
    ReviewReason {
        code: ReasonCode::Uncertain,
        text: ReasonText("why".into()),
    }
}

fn outcomes() -> Vec<Result<ReviewVerdict, ReviewError>> {
    vec![
        Ok(ReviewVerdict::Allow),
        Ok(ReviewVerdict::Ask { why: reason() }),
        Ok(ReviewVerdict::Deny { why: reason() }),
        Err(ReviewError::Timeout),
        Err(ReviewError::Unparseable),
    ]
}

/// Every verdict (or none) for each of the three stages.
fn every_combination() -> Vec<Vec<(Stage, Result<ReviewVerdict, ReviewError>)>> {
    let stages = [Stage::Quick, Stage::Deliberate, Stage::SecondOpinion];
    let mut all = vec![vec![]];
    for stage in stages {
        let mut next = Vec::new();
        for prefix in &all {
            next.push(prefix.clone());
            for o in outcomes() {
                let mut with = prefix.clone();
                with.push((stage, o));
                next.push(with);
            }
        }
        all = next;
    }
    all
}

#[test]
fn tighten_never_loosens() {
    let plans: [&[Stage]; 4] = [
        &[],
        &[Stage::Quick],
        &[Stage::Quick, Stage::Deliberate],
        &[Stage::Quick, Stage::Deliberate, Stage::SecondOpinion],
    ];
    let mut checked = 0;
    for (name, ruling) in rulings() {
        for planned in plans {
            for verdicts in every_combination() {
                let gate = tighten(&ruling, planned, &verdicts);
                checked += 1;
                let all_planned_allowed = !planned.is_empty()
                    && planned.iter().all(|s| {
                        verdicts
                            .iter()
                            .any(|(vs, v)| vs == s && matches!(v, Ok(ReviewVerdict::Allow)))
                    });
                let any_deny = verdicts
                    .iter()
                    .any(|(_, v)| matches!(v, Ok(ReviewVerdict::Deny { .. })));
                match (&ruling, gate) {
                    (Ruling::Deny(_), g) => {
                        assert_eq!(g, Gate::Refuse(DenyCode::NotAllowed), "{name}")
                    }
                    (Ruling::Ask(_), g) => assert_eq!(g, Gate::Confirm, "{name}"),
                    (Ruling::AllowFinal(_), g) => assert_eq!(g, Gate::Run, "{name}"),
                    (Ruling::AllowJudged(_), Gate::Run) => {
                        assert!(
                            all_planned_allowed && !any_deny,
                            "judged ran without every planned stage allowing: {planned:?} {verdicts:?}"
                        );
                    }
                    (Ruling::AllowJudged(_), Gate::Refuse(_)) => {
                        assert!(any_deny, "refused without a deny: {verdicts:?}")
                    }
                    (Ruling::AllowJudged(_), Gate::Confirm) => {
                        assert!(!all_planned_allowed || any_deny || planned.is_empty())
                    }
                }
            }
        }
    }
    assert_eq!(checked, 4 * 4 * 216);
}

#[test]
fn a_missing_review_or_an_error_never_allows() {
    let judged = Ruling::AllowJudged(vec![]);
    let planned = [Stage::Quick];
    assert_eq!(tighten(&judged, &planned, &[]), Gate::Confirm);
    assert_eq!(
        tighten(
            &judged,
            &planned,
            &[(Stage::Quick, Err(ReviewError::Timeout))]
        ),
        Gate::Confirm
    );
    assert_eq!(
        tighten(&judged, &[], &[(Stage::Quick, Ok(ReviewVerdict::Allow))]),
        Gate::Confirm
    );
    assert_eq!(
        tighten(
            &judged,
            &planned,
            &[(Stage::Quick, Ok(ReviewVerdict::Allow))]
        ),
        Gate::Run
    );
}

#[test]
fn every_error_kind_confirms() {
    for error in [
        ReviewError::Timeout,
        ReviewError::Unavailable,
        ReviewError::Unparseable,
        ReviewError::OutOfVocabulary,
    ] {
        let gate = tighten(
            &Ruling::AllowJudged(vec![]),
            &[Stage::Quick],
            &[(Stage::Quick, Err(error))],
        );
        assert_eq!(gate, Gate::Confirm, "{error:?}");
    }
}

#[test]
fn cascade_plan_table() {
    let judged = Ruling::AllowJudged(vec![]);
    let cases: Vec<(&str, Ruling, Impact, Vec<Stage>)> = vec![
        (
            "judged, low",
            judged.clone(),
            Impact::Low,
            vec![Stage::Quick],
        ),
        (
            "judged, high",
            judged,
            Impact::High,
            vec![Stage::Quick, Stage::Deliberate, Stage::SecondOpinion],
        ),
        (
            "final runs no stage",
            Ruling::AllowFinal(vec![]),
            Impact::High,
            vec![],
        ),
        (
            "ask runs no stage",
            Ruling::Ask(vec![]),
            Impact::High,
            vec![],
        ),
        (
            "deny runs no stage",
            Ruling::Deny(vec![]),
            Impact::Low,
            vec![],
        ),
    ];
    for (name, ruling, impact, want) in cases {
        assert_eq!(plan(&ruling, impact), want, "case: {name}");
    }
}

#[test]
fn a_quick_flag_on_a_low_impact_call_adds_the_deliberate_stage_but_still_cannot_run() {
    let flag = Ok(ReviewVerdict::Ask { why: reason() });
    let planned = escalate(&[Stage::Quick], &flag);
    assert_eq!(planned, vec![Stage::Quick, Stage::Deliberate]);
    assert_eq!(
        escalate(&[Stage::Quick], &Ok(ReviewVerdict::Allow)),
        vec![Stage::Quick]
    );
    assert_eq!(
        escalate(
            &[Stage::Quick, Stage::Deliberate, Stage::SecondOpinion],
            &flag
        )
        .len(),
        3
    );
    let verdicts = [
        (Stage::Quick, flag),
        (Stage::Deliberate, Ok(ReviewVerdict::Allow)),
    ];
    assert_eq!(
        tighten(&Ruling::AllowJudged(vec![]), &planned, &verdicts),
        Gate::Confirm
    );
}

fn choice(family: &str) -> ModelChoice {
    ModelChoice {
        account: AccountId::parse("local").expect("account"),
        model: ModelId::parse("m").expect("model"),
        family: ModelFamily(family.into()),
    }
}

#[test]
fn the_second_opinion_must_come_from_another_family() {
    let set = |d: &str, s: &str| ReviewerSet {
        quick: choice("tiny"),
        deliberate: choice(d),
        second: choice(s),
    };
    assert_eq!(set("qwen", "qwen").check(), Err(SetFault::SameFamily));
    assert_eq!(set("qwen", "llama").check(), Ok(()));
}

#[test]
fn verdict_shapes_are_a_token_for_quick_and_a_record_for_the_rest() {
    use model_provider::Shape;
    let Shape::Choice(words) = verdict_shape(Stage::Quick) else {
        panic!("a choice")
    };
    assert_eq!(
        words.iter().map(|w| w.0.as_str()).collect::<Vec<_>>(),
        ["pass", "flag"]
    );
    for stage in [Stage::Deliberate, Stage::SecondOpinion] {
        let Shape::Record(fields) = verdict_shape(stage) else {
            panic!("a record")
        };
        let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["verdict", "code", "reason"]);
        let Shape::Choice(codes) = &fields[1].shape else {
            panic!("codes")
        };
        assert!(codes.iter().any(|c| c.0 == "injection_suspected"));
        assert_eq!(codes.len(), 11);
    }
}
