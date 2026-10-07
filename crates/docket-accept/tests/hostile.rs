//! The hostile-model corpus in the gate: the planner cases over their cassettes with the real
//! companion, intentd and the apps; and the spoiled writer and reviewer replies through the real
//! writer and cascade over inferd's replay engine.

mod support;

use docket_accept::live::hostile::run_planner_case;
use docket_accept::live::{CorpusOptions, Engine, Timeouts, run_corpus_live};
use docket_accept::world::ModelSource;
use docket_core::{ExchangeAnswer, Millis};
use docket_eval::{Corpus, Judgement, load_all, load_planner_cases};
use std::path::PathBuf;
use std::time::Duration;
use support::*;

fn eval_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../eval")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_planner_case_ends_safely_on_its_cassette() {
    let cases = load_planner_cases(&eval_root().join("hostile-model/planner")).expect("cases");
    assert!(cases.len() >= 25);
    let mut broken = Vec::new();
    for case in &cases {
        let model = ModelSource::Scripted(case.cassette.clone());
        let report = run_planner_case(
            &binaries(),
            case,
            &model,
            (None, None),
            Duration::from_secs(120),
        )
        .await;
        if !report.failures.is_empty() {
            broken.push(format!(
                "{}: {:?}\n{}\n{}",
                report.id, report.failures, report.transcript, report.logs
            ));
        }
    }
    assert!(broken.is_empty(), "{}", broken.join("\n----\n"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn spoiled_replies_reach_the_stage_that_reads_them_and_the_case_still_holds() {
    let ids = [
        (
            "hostile-model-reviewer-deliberate-trailing-text",
            "Thanks for checking.",
        ),
        ("hostile-model-reviewer-quick-uppercase", "PASS"),
        (
            "hostile-model-reviewer-second-trailing-text",
            "I agree with the others.",
        ),
        ("hostile-model-writer-prose", "Sure! Here is the policy"),
    ];
    let cases: Vec<_> = load_all(&eval_root())
        .expect("corpus")
        .into_iter()
        .filter(|c| c.corpus == Corpus::HostileModel)
        .collect();
    assert!(cases.len() >= 50, "{} hostile cases", cases.len());
    let out = tempfile::tempdir().expect("scratch");
    let outcome = run_corpus_live(
        &binaries(),
        &cases,
        &CorpusOptions {
            label: "hostile".to_owned(),
            engine: Engine::Scripted,
            inferd_config: None,
            cassette: None,
            timeouts: Timeouts::Every(Millis(600_000)),
            out: out.path().to_owned(),
            accountd: None,
            catalog: None,
        },
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        outcome.note.missed.is_empty(),
        "missed: {:?}\n{}",
        outcome.note.missed,
        outcome
            .traces
            .iter()
            .filter(|(_, t)| t.judgement == Judgement::Missed)
            .map(|(_, t)| t.render())
            .collect::<String>()
    );
    for (id, words) in ids {
        let (_, trace) = outcome
            .traces
            .iter()
            .find(|(_, t)| t.id.0 == id)
            .unwrap_or_else(|| panic!("no trace for {id}"));
        let said = trace
            .setup_exchanges
            .iter()
            .chain(trace.steps.iter().flat_map(|s| s.exchanges.iter()))
            .any(|e| match &e.answer {
                ExchangeAnswer::Replied { text, .. } => text.contains(words),
                _ => false,
            });
        assert!(
            said,
            "{id}: the model never said {words:?}\n{}",
            trace.render()
        );
    }
}
