//! `docket-live`: the live-eval harness. `corpus` plays the eval corpora through the router with
//! the real policy writer and reviewer cascade over a model; `smoke` plays the acceptance flows
//! with every daemon against a model. The model is `--engine scripted|local|cloud`. It runs on a
//! private bus with scratch HOME and XDG directories; the network is touched only by
//! `--engine cloud`, and that is said before anything starts. Start it through
//! `scripts/eval-release.sh` or `dev/live-smoke.sh`.

use docket_accept::live::cli::{Command, CorpusArgs, SmokeArgs, UsageError, parse};
use docket_accept::live::flows::{Flow, Kind, run_flow};
use docket_accept::live::regress;
use docket_accept::live::{
    CorpusOptions, Engine, EngineError, Reach, Timeouts, packaged_binaries, run_corpus_live,
};
use docket_accept::world::ModelSource;
use docket_core::Millis;
use docket_eval::{Case, Corpus, RunReport, load_all};
use std::process::ExitCode;
use std::time::Duration;

const LIVE_BOUND_MS: u32 = 30_000;

fn say_reach(engine: Engine) {
    if engine.reach() == Reach::Network {
        eprintln!(
            "docket-live: --engine cloud: prompts WILL be sent over the network to the hosted model your inferd config names"
        );
    } else {
        eprintln!("docket-live: --engine {}: no network", engine.slug());
    }
}

fn corpus_dir_name(corpus: Corpus) -> String {
    serde_json::to_string(&corpus)
        .unwrap_or_default()
        .trim_matches('"')
        .replace('_', "-")
}

fn chosen(args: &CorpusArgs, all: Vec<Case>) -> Vec<Case> {
    all.into_iter()
        .filter(|c| args.cases.is_empty() || args.cases.contains(&c.id.0))
        .filter(|c| args.corpora.is_empty() || args.corpora.contains(&corpus_dir_name(c.corpus)))
        .collect()
}

fn timeouts(args: &CorpusArgs) -> Timeouts {
    match (args.timeout_ms, args.engine) {
        (Some(ms), _) => Timeouts::Every(Millis(ms)),
        (None, Engine::Scripted) => Timeouts::Shipped,
        (None, _) => Timeouts::Every(Millis(LIVE_BOUND_MS)),
    }
}

fn over_target(report: &RunReport, max: u32) -> Vec<String> {
    report
        .per_corpus
        .iter()
        .filter(|(_, m)| m.fnr.high.0 > max)
        .map(|(corpus, m)| format!("{corpus:?}: FNR upper bound {} > {max}", m.fnr.high.0))
        .collect()
}

async fn corpus(args: CorpusArgs) -> Result<ExitCode, String> {
    say_reach(args.engine);
    let binaries = packaged_binaries().map_err(|e| e.to_string())?;
    let mut options = CorpusOptions {
        label: args.label.clone(),
        engine: args.engine,
        inferd_config: args.inferd_config.clone(),
        cassette: None,
        timeouts: timeouts(&args),
        out: args.out.clone(),
        accountd: args.accountd.clone(),
    };
    let mut misses = Vec::new();
    let mut outcome = None;
    let all = load_all(&args.eval_dir).map_err(|e| e.to_string())?;
    match &args.regress {
        Some(dir) => {
            for pair in regress::load(dir, &all).map_err(|e| e.to_string())? {
                options.cassette = Some(pair.cassette.clone());
                options.out = args.out.join(&pair.case.id.0);
                let ran = run_corpus_live(&binaries, std::slice::from_ref(&pair.case), &options)
                    .await
                    .map_err(|e| e.to_string())?;
                misses.extend(ran.note.missed.clone());
                outcome = Some(ran);
            }
        }
        None => {
            let cases = chosen(&args, all);
            if cases.is_empty() {
                return Err("no case matches --corpus/--case".to_owned());
            }
            let ran = run_corpus_live(&binaries, &cases, &options)
                .await
                .map_err(|e| e.to_string())?;
            misses.extend(ran.note.missed.clone());
            outcome = Some(ran);
        }
    }
    let Some(outcome) = outcome else {
        return Err("nothing to run".to_owned());
    };
    let text = outcome.report.render(&outcome.note);
    print!("{text}");
    if let Some(path) = &args.report {
        std::fs::write(path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
        eprintln!("docket-live: report written to {}", path.display());
    }
    eprintln!(
        "docket-live: traces in {} (index.txt, one .trace.txt, .cassette.jsonl and .case.toml per case)",
        outcome.trace_dir.display()
    );
    let over = args
        .fnr_max_permille
        .map(|max| over_target(&outcome.report, max))
        .unwrap_or_default();
    over.iter()
        .for_each(|line| eprintln!("docket-live: {line}"));
    Ok(match (misses.is_empty(), over.is_empty()) {
        (true, true) => ExitCode::SUCCESS,
        _ => ExitCode::from(1),
    })
}

async fn smoke(args: SmokeArgs) -> Result<ExitCode, String> {
    say_reach(args.engine);
    let binaries = packaged_binaries().map_err(|e| e.to_string())?;
    let flows: Vec<Flow> = if args.flows.is_empty() {
        Flow::ALL.to_vec()
    } else {
        args.flows
            .iter()
            .map(|f| Flow::parse(f).ok_or_else(|| format!("no flow named {f:?}")))
            .collect::<Result<_, _>>()?
    };
    let traces = args.out.join("smoke");
    std::fs::create_dir_all(&traces).map_err(|e| e.to_string())?;
    let mut failed = false;
    for flow in flows {
        let model: ModelSource = args
            .engine
            .source(flow.cassette().to_owned(), args.inferd_config.as_deref())
            .map_err(|e: EngineError| e.to_string())?;
        let report = run_flow(
            &binaries,
            flow,
            &model,
            Some(args.out.join("scratch")),
            Duration::from_secs(args.patience_s),
        )
        .await;
        let file = traces.join(format!("{}.trace.txt", flow.slug()));
        std::fs::write(&file, &report.transcript).map_err(|e| e.to_string())?;
        let verdict = if report.failures.is_empty() {
            "PASS"
        } else {
            "FAIL"
        };
        println!("{verdict} {}  trace: {}", flow.slug(), file.display());
        for f in &report.failures {
            let kind = match f.kind {
                Kind::Safety => "safety",
                Kind::Capability => "capability",
            };
            println!("     [{kind}] {}", f.what);
        }
        failed |= !report.failures.is_empty();
    }
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let command = match parse(&words) {
        Ok(command) => command,
        Err(why @ UsageError::Subcommand) => {
            eprintln!("{why}");
            return ExitCode::from(2);
        }
        Err(why) => {
            eprintln!("docket-live: {why}");
            return ExitCode::from(2);
        }
    };
    let ran = match command {
        Command::Corpus(args) => corpus(args).await,
        Command::Smoke(args) => smoke(args).await,
    };
    ran.unwrap_or_else(|why| {
        eprintln!("docket-live: {why}");
        ExitCode::from(2)
    })
}
