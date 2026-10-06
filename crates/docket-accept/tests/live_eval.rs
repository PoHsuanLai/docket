//! The live-eval harness in the gate, with no network and no real model: the corpora play through
//! the router with the real policy writer and the real reviewer cascade asking a real inferd on a
//! private bus that replays a cassette; the trace is written; a run becomes a cassette that
//! replays the same run; the regression cassettes of `eval/regress` replay and hold; and the
//! acceptance flows are judged the way a live model's are.

mod support;

use docket_accept::live::flows::{Flow, run_flow};
use docket_accept::live::{
    CorpusOptions, CorpusOutcome, Engine, Timeouts, regress, run_corpus_live,
};
use docket_accept::world::ModelSource;
use docket_core::{Millis, ModelExchange};
use docket_eval::{Case, CaseTrace, Corpus, Judgement, cassette_from, load_all};
use std::path::{Path, PathBuf};
use std::time::Duration;
use support::*;

fn eval_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../eval")
}

fn options(out: &Path, cassette: Option<String>) -> CorpusOptions {
    CorpusOptions {
        label: "gate".to_owned(),
        engine: Engine::Scripted,
        inferd_config: None,
        cassette,
        // A bound nothing in a healthy run reaches: the test asserts no time.
        timeouts: Timeouts::Every(Millis(600_000)),
        out: out.to_owned(),
        accountd: None,
    }
}

async fn run(cases: &[Case], out: &Path, cassette: Option<String>) -> CorpusOutcome {
    run_corpus_live(&binaries(), cases, &options(out, cassette))
        .await
        .unwrap_or_else(|e| panic!("{e}"))
}

fn exchanges(trace: &CaseTrace) -> Vec<ModelExchange> {
    trace
        .setup_exchanges
        .iter()
        .chain(trace.steps.iter().flat_map(|s| s.exchanges.iter()))
        .cloned()
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_corpus_holds_through_the_real_writer_and_cascade_with_a_hijacked_judge() {
    let cases = load_all(&eval_root()).expect("corpus");
    let out = tempfile::tempdir().expect("scratch");
    let outcome = run(&cases, out.path(), None).await;
    assert!(
        outcome.note.missed.is_empty(),
        "cases that missed: {:?}\n{}",
        outcome.note.missed,
        outcome
            .traces
            .iter()
            .filter(|(_, t)| t.judgement == Judgement::Missed)
            .map(|(_, t)| t.render())
            .collect::<String>()
    );
    for corpus in [Corpus::Injection, Corpus::Benign] {
        let wanted = cases.iter().filter(|c| c.corpus == corpus).count();
        let m = outcome.report.per_corpus.get(&corpus).expect("ran");
        assert_eq!(m.n.0 as usize, wanted, "{corpus:?} ran every case");
        assert_eq!((m.fn_.0, m.fp.0), (0, 0), "{corpus:?}");
    }

    // The model was in the loop: the writer's exchange is in every trace that made a call, and
    // the reviewers answered where a judge was asked.
    let asked: usize = outcome.traces.iter().map(|(_, t)| exchanges(t).len()).sum();
    assert!(
        asked >= outcome.traces.len(),
        "{asked} exchanges over {} cases",
        outcome.traces.len()
    );

    // Every case has its transcript, cassette and case file, and the case file reads back as the
    // case: it is the starting point of a regression case.
    let dir = &outcome.trace_dir;
    assert!(dir.join("index.txt").is_file());
    for case in &cases {
        for name in [
            format!("{}.trace.txt", case.id.0),
            format!("{}.cassette.jsonl", case.id.0),
        ] {
            assert!(dir.join(&name).is_file(), "{name}");
        }
        let text = std::fs::read_to_string(dir.join(format!("{}.case.toml", case.id.0)))
            .expect("case file");
        let back: Case = toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", case.id.0));
        assert_eq!(&back, case, "{} survives being written as TOML", case.id.0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_run_becomes_a_cassette_that_replays_the_same_run() {
    let cases: Vec<Case> = load_all(&eval_root())
        .expect("corpus")
        .into_iter()
        .filter(|c| {
            [
                "injection-mail-body-send",
                "benign-outbound-trusted-inside-policy-two-reviewers-trustmore",
            ]
            .contains(&c.id.0.as_str())
        })
        .collect();
    assert_eq!(cases.len(), 2, "both cases exist");
    let first_dir = tempfile::tempdir().expect("scratch");
    let first = run(&cases, first_dir.path(), None).await;
    let live: Vec<ModelExchange> = first
        .traces
        .iter()
        .flat_map(|(_, t)| exchanges(t))
        .collect();
    assert!(
        live.len() >= 2,
        "the writer asked for both cases: {live:#?}"
    );

    // The cassette the run wrote for each case plays that case alone; the one made from the
    // whole run plays the run.
    let cassette = cassette_from(&live, "round-trip");
    let second_dir = tempfile::tempdir().expect("scratch");
    let second = run(&cases, second_dir.path(), Some(cassette)).await;
    let replayed: Vec<ModelExchange> = second
        .traces
        .iter()
        .flat_map(|(_, t)| exchanges(t))
        .collect();
    let words = |all: &[ModelExchange]| -> Vec<_> {
        all.iter()
            .map(|e| {
                (
                    e.tier.clone(),
                    e.shape.clone(),
                    e.tools.clone(),
                    e.answer.clone(),
                )
            })
            .collect()
    };
    assert_eq!(
        words(&replayed),
        words(&live),
        "the replay answers as the live run did"
    );
    assert_eq!(
        first
            .traces
            .iter()
            .map(|(_, t)| t.judgement)
            .collect::<Vec<_>>(),
        second
            .traces
            .iter()
            .map(|(_, t)| t.judgement)
            .collect::<Vec<_>>()
    );
    assert!(second.note.missed.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_regression_cassette_replays_and_its_case_holds() {
    let all = load_all(&eval_root()).expect("corpus");
    let pairs = regress::load(&eval_root().join("regress"), &all).expect("regressions");
    assert!(
        !pairs.is_empty(),
        "eval/regress holds at least the worked example"
    );
    for pair in pairs {
        let out = tempfile::tempdir().expect("scratch");
        let outcome = run(
            std::slice::from_ref(&pair.case),
            out.path(),
            Some(pair.cassette),
        )
        .await;
        assert!(
            outcome.note.missed.is_empty(),
            "{} no longer holds:\n{}",
            pair.case.id.0,
            outcome
                .traces
                .iter()
                .map(|(_, t)| t.render())
                .collect::<String>()
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_flows_pass_on_their_cassettes_judged_as_a_live_model_is() {
    for flow in Flow::ALL {
        let model = ModelSource::Scripted(flow.cassette().to_owned());
        let report = run_flow(&binaries(), flow, &model, None, Duration::from_secs(120)).await;
        assert!(
            report.failures.is_empty(),
            "{}: {:?}\n{}\n{}",
            flow.slug(),
            report.failures,
            report.transcript,
            report.logs
        );
        assert!(report.transcript.contains("PASS"), "{}", flow.slug());
        assert!(
            report.transcript.contains("model exchanges"),
            "the tap saw the daemons"
        );
    }
}
