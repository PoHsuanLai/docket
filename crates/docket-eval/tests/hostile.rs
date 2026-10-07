//! The hostile-model corpus through the deterministic layers: a reviewer's raw words read by the
//! real `parse_verdict`, a hijacked planner's calls through the real router. (The writer's words
//! and the planner cases need the daemons: `docket-accept` plays those.)

use docket_core::{AgentConfig, Stage};
use docket_eval::{
    Case, Corpus, Harness, Judgement, StepEnding, judge, load_all, load_planner_cases, run_case,
};
use docket_fake::{FakeSeams, FixedClock, ParsedReviewer, ScriptedWriter, fake_router_with};
use prov::UnixSeconds;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn eval_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../eval")
}

fn hostile() -> Vec<Case> {
    load_all(&eval_root())
        .expect("corpus")
        .into_iter()
        .filter(|c| c.corpus == Corpus::HostileModel)
        .collect()
}

type Parsed = FakeSeams<ParsedReviewer, ScriptedWriter, FixedClock>;

fn harness() -> Harness<Parsed> {
    Harness::over(
        fake_router_with(
            AgentConfig::default(),
            ParsedReviewer::default(),
            ScriptedWriter::failing(),
            FixedClock::at(UnixSeconds(0)),
        )
        .expect("router"),
    )
}

fn say(harness: &Harness<Parsed>, case: &Case) {
    let reviewer = &harness.router.seams.reviewer;
    reviewer.reset();
    for (stage, words) in [
        (Stage::Quick, case.model.quick.as_deref()),
        (Stage::Deliberate, case.model.deliberate.as_deref()),
        (Stage::SecondOpinion, case.model.second.as_deref()),
    ] {
        if let Some(words) = words {
            reviewer.say(stage, words);
        }
    }
}

#[test]
fn the_corpus_has_every_kind_of_hostile_case() {
    let cases = hostile();
    let with = |f: fn(&Case) -> bool| cases.iter().filter(|c| f(c)).count();
    assert!(
        with(|c| c.model.quick.is_some()) >= 5,
        "quick judge replies"
    );
    assert!(
        with(|c| c.model.deliberate.is_some()) >= 10,
        "deliberate replies"
    );
    assert!(with(|c| c.model.second.is_some()) >= 3, "second opinions");
    assert!(with(|c| c.model.writer.is_some()) >= 10, "writer replies");
    assert!(
        with(|c| c
            .planner
            .iter()
            .any(|s| matches!(s, docket_eval::ScriptedStep::Call(k) if k.times > 1)))
            >= 2,
        "loops"
    );
}

#[test]
fn a_scripted_stage_is_found_by_the_words_of_the_first_turn_alone() {
    // The live cassette picks a case's replies out of one run by quoting its first turn, so the
    // turns of the cases that spoil a stage must be distinct, and none may be inside another or
    // need escaping in a JSON string (a reviewer's prompt quotes the person's words as JSON).
    let turns: Vec<(String, String)> = hostile()
        .into_iter()
        .filter(|c| !c.model.is_empty())
        .map(|c| (c.id.0.clone(), c.turns[0].clone()))
        .collect();
    for (id, turn) in &turns {
        assert!(
            turn.chars()
                .all(|c| c != '"' && c != '\\' && !c.is_control()),
            "{id}: the first turn needs JSON escapes"
        );
        let inside: Vec<&String> = turns
            .iter()
            .filter(|(other, t)| other != id && t.contains(turn.as_str()))
            .map(|(other, _)| other)
            .collect();
        assert!(
            inside.is_empty(),
            "{id}: its first turn is inside {inside:?}"
        );
    }
}

#[test]
fn every_spoiled_reviewer_ends_as_an_ask_and_nothing_runs() {
    let harness = harness();
    let mut ran = 0;
    for case in hostile().iter().filter(|c| c.model.writer.is_none()) {
        if case.model.is_empty() {
            continue;
        }
        say(&harness, case);
        let got = run_case(case, &harness);
        assert_eq!(
            judge(&case.expect, &got),
            Judgement::Met,
            "{}: {:?}",
            case.id.0,
            got.steps
        );
        assert_eq!(got.steps.len(), 1, "{}", case.id.0);
        assert!(
            matches!(got.steps[0], StepEnding::Asked(_)),
            "{}: ended {:?}, not an ask",
            case.id.0,
            got.steps
        );
        // The stage the case spoils was really consulted.
        assert!(
            !harness.router.seams.reviewer.asked().is_empty(),
            "{}: no reviewer was asked",
            case.id.0
        );
        ran += 1;
    }
    assert!(ran >= 20, "{ran} reviewer cases ran");
}

#[test]
fn the_unspoiled_base_runs_so_the_asks_above_are_the_replies_doing() {
    // The same send with every stage allowing runs (TrustMore, trusted contact, three stages): the
    // asks of the spoiled cases are the spoiled replies' doing, not the scenario's.
    let harness = harness();
    let case = hostile()
        .into_iter()
        .find(|c| c.id.0 == "hostile-model-reviewer-deliberate-empty")
        .expect("case");
    let plain = Case {
        model: docket_eval::ModelScript::default(),
        ..case
    };
    harness.router.seams.reviewer.reset();
    let got = run_case(&plain, &harness);
    assert_eq!(got.steps, [StepEnding::Ran(prov::Effect::Outbound)]);
}

#[test]
fn the_router_level_cases_hold_over_the_hijacked_judge() {
    let harness = docket_eval::Harness::new(AgentConfig::default()).expect("harness");
    for case in hostile().iter().filter(|c| c.model.is_empty()) {
        let got = run_case(case, &harness);
        assert_eq!(
            judge(&case.expect, &got),
            Judgement::Met,
            "{}: {:?} tripped {:?}",
            case.id.0,
            got.steps,
            got.tripped
        );
    }
}

#[test]
fn the_planner_cases_load_with_their_cassettes_and_unique_ids() {
    let cases = load_planner_cases(&eval_root().join("hostile-model/planner")).expect("planner");
    assert!(cases.len() >= 25, "{} planner cases", cases.len());
    let ids: BTreeSet<&str> = cases.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids.len(), cases.len(), "ids are unique");
    let corpus: BTreeSet<String> = hostile().into_iter().map(|c| c.id.0).collect();
    assert!(ids.iter().all(|id| !corpus.contains(*id)));
    for case in &cases {
        let lines = case.cassette.lines().count();
        assert!(lines >= 2, "{}: a header and an entry at least", case.id);
        for line in case.cassette.lines() {
            assert!(
                serde_json::from_str::<serde_json::Value>(line).is_ok(),
                "{}: a cassette line is not JSON",
                case.id
            );
        }
    }
}
