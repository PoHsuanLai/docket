//! `docket-live`: the live-eval harness. `corpus` plays the eval corpora through the router with
//! the real policy writer and reviewer cascade over a model; `smoke` plays the acceptance flows
//! with every daemon against a model. The model is `--engine scripted|local|cloud`. It runs on a
//! private bus with scratch HOME and XDG directories; the network is touched only by
//! `--engine cloud`, and that is said before anything starts. Start it through
//! `scripts/eval-release.sh` or `dev/live-smoke.sh`.

use docket_accept::live::acp::{
    AcpSpec, Redactor, Task, agent_cassette, list_models_acp, run_flow_acp,
};
use docket_accept::live::attack_planner::run_attacks_planner;
use docket_accept::live::cli::{
    Agent, Command, CorpusArgs, PlannerMode, SmokeArgs, UsageError, parse,
};
use docket_accept::live::flows::{Flow, Kind, run_flow};
use docket_accept::live::hostile::run_planner_case;
use docket_accept::live::keys_home::KeysHome;
use docket_accept::live::report_file::write_report;
use docket_accept::live::{
    CorpusOptions, Engine, EngineError, Reach, Timeouts, packaged_binaries, run_corpus_live,
};
use docket_accept::live::{catalog, regress};
use docket_accept::world::{Binaries, ModelSource};
use docket_core::Millis;
use docket_eval::{
    PlannerCase, load_all, load_attacks, load_planner_cases, stamp_summary, write_summary,
};
use eval_core::{
    AttackCase, AttackMetrics, Case, Corpus, LiveMetrics, RunNote, RunReport, planner_slug,
    score_traces, utc_date,
};
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

/// What an external agent reaches, said before anything starts: its own network, whatever the
/// engine is, and the login it is given.
fn say_agent(agent: &Agent) {
    if let Agent::Acp(spec) = agent {
        eprintln!(
            "docket-live: --agent acp: {} runs in a sandbox with network {}{}",
            spec.program,
            docket_accept::live::acp::spec::network_word(spec.network),
            if spec.network == bulkhead::NetworkMode::Host {
                " (its prompts go to its own provider)"
            } else {
                ""
            }
        );
        if spec.credentials.is_some() {
            eprintln!(
                "docket-live: the login you named is copied into the scratch HOME for this run and removed when it ends"
            );
        }
    }
}

fn corpus_dir_name(corpus: Corpus) -> String {
    serde_json::to_string(&corpus)
        .unwrap_or_default()
        .trim_matches('"')
        .replace('_', "-")
}

/// The cases a run is asked for. Unnamed, the hostile-model corpus runs only on a cassette: its
/// cases are the spoiled replies themselves, and a live model answers in its own words instead.
fn chosen(args: &CorpusArgs, all: Vec<Case>) -> Vec<Case> {
    let unnamed = args.cases.is_empty() && args.corpora.is_empty();
    all.into_iter()
        .filter(|c| args.cases.is_empty() || args.cases.contains(&c.id.0))
        .filter(|c| args.corpora.is_empty() || args.corpora.contains(&corpus_dir_name(c.corpus)))
        .filter(|c| {
            !unnamed
                || matches!(args.engine, Engine::Scripted)
                || !matches!(c.corpus, Corpus::HostileModel)
        })
        .collect()
}

/// What `--corpus` names to play the attack suite (`eval/injection/attacks`): AgentDojo's utility
/// without attack, utility under attack and targeted attack success rate, in the report.
const ATTACKS: &str = "attacks";

/// The attacks a run is asked for: those of `--corpus attacks`, or the ones `--case` names (by the
/// attack's id or the id of either case it compiles to). Each is played as its pair of cases.
fn chosen_attacks(args: &CorpusArgs) -> Result<Vec<AttackCase>, String> {
    let named = args.corpora.iter().any(|c| c == ATTACKS);
    if !named && args.cases.is_empty() {
        return Ok(Vec::new());
    }
    let all =
        load_attacks(&args.eval_dir.join("injection").join(ATTACKS)).map_err(|e| e.to_string())?;
    Ok(all
        .into_iter()
        .filter(|a| match args.cases.is_empty() {
            true => named,
            false => args
                .cases
                .iter()
                .any(|c| *c == a.id.0 || *c == a.clean_id().0 || *c == a.attacked_id().0),
        })
        .collect())
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

/// Every case of the chosen corpora, as a report that says none of them could run with an agent.
/// A case scripts the planner's calls and judges the router's ruling on each; an external agent
/// chooses its own, so the case's expectation has nothing to compare. The flows (`smoke`) are what
/// an agent is measured on.
fn corpus_agent(args: &CorpusArgs, spec: &AcpSpec) -> Result<ExitCode, String> {
    let all = load_all(&args.eval_dir).map_err(|e| e.to_string())?;
    let cases = chosen(args, all);
    if cases.is_empty() {
        return Err("no case matches --corpus/--case".to_owned());
    }
    let why = "the case scripts the planner's calls and judges the router's ruling on each; an external agent chooses its own";
    let note = RunNote {
        label: args.label.clone(),
        engine: format!("{} + acp:{}", args.engine.slug(), spec.program),
        skipped: cases
            .iter()
            .map(|c| (c.id.0.clone(), why.to_owned()))
            .collect(),
        missed: Vec::new(),
        catalogue: "catalogue: not used (no model is asked)".to_owned(),
    };
    let report = RunReport {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        per_corpus: Default::default(),
        attack: None,
    };
    let text = report.render(&note);
    print!("{text}");
    if let Some(path) = &args.report {
        write_report(path, &text)?;
        eprintln!("docket-live: report written to {}", path.display());
    }
    Ok(ExitCode::SUCCESS)
}

/// Today in UTC, as `YYYY-MM-DD`: the one clock read of the summary, made here in the binary.
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    utc_date(secs)
}

/// Keeps the attack suite's numbers of this run under `dir`, named by day and commit.
fn write_attack_summary(
    args: &CorpusArgs,
    outcomes: &[docket_accept::live::CorpusOutcome],
    dir: &std::path::Path,
) -> Result<(), String> {
    let Some(metrics) = outcomes.iter().find_map(|o| o.report.attack.clone()) else {
        return Err("--attack-summary: this run played no attacks (--corpus attacks)".to_owned());
    };
    let summary = stamp_summary(args.engine.slug(), metrics, &args.eval_dir, &today())
        .map_err(|e| e.to_string())?;
    let path = write_summary(&summary, dir).map_err(|e| e.to_string())?;
    eprintln!("docket-live: attack summary written to {}", path.display());
    Ok(())
}

/// `--planner live`: the attack suite played by the companion's real planner (`--corpus attacks`).
/// The report is the usual header and a section of its own for the live planner's numbers; the
/// per-commit summary, when asked for, is a file of its own named for the planner model. A live
/// run is a measurement, not a gate: only an attack the harness could not play fails it.
async fn corpus_live_planner(
    args: &CorpusArgs,
    binaries: &Binaries,
    options: &CorpusOptions,
) -> Result<ExitCode, String> {
    if args.regress.is_some() || args.corpora.iter().any(|c| c != ATTACKS) {
        return Err(format!(
            "--planner live plays the attack suite only: name --corpus {ATTACKS} (or --case <attack id>), and no other corpus or --regress"
        ));
    }
    let attacks = chosen_attacks(args)?;
    if attacks.is_empty() {
        return Err(format!(
            "--planner live plays the attack suite: name --corpus {ATTACKS} (or --case <attack id>)"
        ));
    }
    eprintln!(
        "docket-live: --planner live: {} attacks, two runs each, played by the companion's own planner model (a run takes a while)",
        attacks.len()
    );
    let ran = run_attacks_planner(binaries, &attacks, options)
        .await
        .map_err(|e| e.to_string())?;
    let metrics = LiveMetrics::from_scores(&ran.planner, &ran.scores);
    let report = RunReport {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        per_corpus: Default::default(),
        attack: None,
    };
    let text = report.render(&ran.note) + &metrics.render();
    print!("{text}");
    if let Some(path) = &args.report {
        write_report(path, &text)?;
        eprintln!("docket-live: report written to {}", path.display());
    }
    if let Some(dir) = &args.attack_summary {
        let engine = format!(
            "{}-live-planner-{}",
            args.engine.slug(),
            planner_slug(&ran.planner)
        );
        let summary = stamp_summary(&engine, metrics.base.clone(), &args.eval_dir, &today())
            .map_err(|e| e.to_string())?;
        let path = write_summary(&summary, dir).map_err(|e| e.to_string())?;
        eprintln!("docket-live: attack summary written to {}", path.display());
    }
    eprintln!(
        "docket-live: traces in {} (index.txt, one .trace.txt, .cassette.jsonl and .case.toml per case)",
        ran.trace_dir.display()
    );
    ran.unscored
        .iter()
        .for_each(|(id, why)| eprintln!("docket-live: {id} was not scored: {why}"));
    Ok(match ran.unscored.is_empty() {
        true => ExitCode::SUCCESS,
        false => ExitCode::from(1),
    })
}

async fn corpus(args: CorpusArgs) -> Result<ExitCode, String> {
    say_reach(args.engine);
    say_agent(&args.agent);
    if let Agent::Acp(spec) = &args.agent {
        return corpus_agent(&args, spec);
    }
    let binaries = packaged_binaries().map_err(|e| e.to_string())?;
    let keys_home = args
        .keys_home
        .as_deref()
        .map(KeysHome::open)
        .transpose()
        .map_err(|e| e.to_string())?;
    let mut options = CorpusOptions {
        label: args.label.clone(),
        engine: args.engine,
        inferd_config: args.inferd_config.clone(),
        cassette: None,
        timeouts: timeouts(&args),
        out: args.out.clone(),
        keys_home,
        catalog: Some(
            args.catalog
                .clone()
                .unwrap_or_else(|| catalog::default_source(&args.eval_dir)),
        ),
        patience: Duration::from_secs(args.patience_s),
    };
    if args.planner == PlannerMode::Live {
        return corpus_live_planner(&args, &binaries, &options).await;
    }
    let mut misses = Vec::new();
    let mut outcomes = Vec::new();
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
                outcomes.push(ran);
            }
        }
        None => {
            let attacks = chosen_attacks(&args)?;
            let mut cases = chosen(&args, all);
            cases.extend(attacks.iter().flat_map(|a| {
                let (clean, attacked) = a.cases();
                [clean, attacked]
            }));
            if cases.is_empty() {
                return Err("no case matches --corpus/--case".to_owned());
            }
            let mut ran = run_corpus_live(&binaries, &cases, &options)
                .await
                .map_err(|e| e.to_string())?;
            if !attacks.is_empty() {
                let scores = score_traces(&attacks, &ran.traces).map_err(|e| e.to_string())?;
                ran.report.attack = Some(AttackMetrics::from_scores(&scores));
            }
            misses.extend(ran.note.missed.clone());
            outcomes.push(ran);
        }
    }
    if outcomes.is_empty() {
        return Err("nothing to run".to_owned());
    }
    // One section per run: a regression pair is a run of its own.
    let text: String = outcomes
        .iter()
        .map(|outcome| outcome.report.render(&outcome.note))
        .collect();
    print!("{text}");
    if let Some(path) = &args.report {
        write_report(path, &text)?;
        eprintln!("docket-live: report written to {}", path.display());
    }
    if let Some(dir) = &args.attack_summary {
        write_attack_summary(&args, &outcomes, dir)?;
    }
    for outcome in &outcomes {
        eprintln!(
            "docket-live: traces in {} (index.txt, one .trace.txt, .cassette.jsonl and .case.toml per case)",
            outcome.trace_dir.display()
        );
    }
    let over: Vec<String> = args
        .fnr_max_permille
        .map(|max| {
            outcomes
                .iter()
                .flat_map(|outcome| over_target(&outcome.report, max))
                .collect()
        })
        .unwrap_or_default();
    over.iter()
        .for_each(|line| eprintln!("docket-live: {line}"));
    Ok(match (misses.is_empty(), over.is_empty()) {
        (true, true) => ExitCode::SUCCESS,
        _ => ExitCode::from(1),
    })
}

/// What a smoke run plays: one of the acceptance flows, or one planner case of the hostile-model
/// corpus.
#[derive(Clone)]
enum Play {
    Flow(Flow),
    Hostile(Box<PlannerCase>),
}

impl Play {
    fn name(&self) -> String {
        match self {
            Play::Flow(flow) => flow.slug().to_owned(),
            Play::Hostile(case) => case.id.clone(),
        }
    }

    fn cassette(&self) -> String {
        match self {
            Play::Flow(flow) => flow.cassette().to_owned(),
            Play::Hostile(case) => case.cassette.clone(),
        }
    }
}

/// The flows and planner cases a run is asked for: the named ones, or else every flow, plus the
/// planner cases when the engine is a cassette. A planner case's expectation is written for its
/// own misbehaving replies; a live model says something else, so on a live engine the cases run
/// only when named.
fn plays(args: &SmokeArgs) -> Result<Vec<Play>, String> {
    let hostile = load_planner_cases(&args.eval_dir.join("hostile-model/planner"))
        .map_err(|e| e.to_string())?;
    let all = Flow::ALL
        .into_iter()
        .map(Play::Flow)
        .chain(hostile.iter().cloned().map(|c| Play::Hostile(Box::new(c))));
    if args.flows.is_empty() {
        return Ok(match args.engine {
            Engine::Scripted => all.collect(),
            _ => Flow::ALL.into_iter().map(Play::Flow).collect(),
        });
    }
    args.flows
        .iter()
        .map(|name| {
            all.clone()
                .find(|p| &p.name() == name)
                .ok_or_else(|| format!("no flow or planner case named {name:?}"))
        })
        .collect()
}

/// The flows with an external agent in the companion's place: the same world, the same flows,
/// the same outcome checks. A planner case of the hostile-model corpus scripts the planner's own
/// replies, so it has no agent counterpart and is said to be skipped.
async fn smoke_agent(args: SmokeArgs, spec: AcpSpec) -> Result<ExitCode, String> {
    let binaries = packaged_binaries().map_err(|e| e.to_string())?;
    let smoke_catalog = args
        .catalog
        .clone()
        .unwrap_or_else(|| catalog::default_source(&args.eval_dir));
    let traces = args.out.join("smoke-acp");
    std::fs::create_dir_all(&traces).map_err(|e| e.to_string())?;
    let patience = Duration::from_secs(args.patience_s);
    if spec.task == Task::ListModels {
        let model = args
            .engine
            .source(agent_cassette(), args.inferd_config.as_deref())
            .map_err(|e: EngineError| e.to_string())?;
        let dirs = (Some(args.out.join("scratch")), Some(smoke_catalog));
        print!("{}", list_models_acp(&binaries, &spec, &model, dirs).await?);
        return Ok(ExitCode::SUCCESS);
    }
    let redactor = spec
        .credentials
        .as_ref()
        .and_then(|c| std::fs::read(&c.from).ok())
        .map_or_else(Redactor::none, |bytes| Redactor::of(&bytes));
    let flows: Vec<Flow> = if args.flows.is_empty() {
        Flow::ALL.to_vec()
    } else {
        args.flows
            .iter()
            .map(|name| {
                Flow::parse(name).ok_or_else(|| {
                    format!("no flow named {name:?} (a planner case has no agent counterpart)")
                })
            })
            .collect::<Result<_, _>>()?
    };
    println!(
        "N/A hostile-model planner cases: they script the planner's replies, which an agent does not take"
    );
    let mut failed = false;
    for flow in flows {
        let model = args
            .engine
            .source(agent_cassette(), args.inferd_config.as_deref())
            .map_err(|e: EngineError| e.to_string())?;
        let dirs = (Some(args.out.join("scratch")), Some(smoke_catalog.clone()));
        let r = run_flow_acp(&binaries, flow, &spec, &model, dirs, patience).await;
        let file = traces.join(format!("{}.trace.txt", flow.slug()));
        let header = format!("{}\n\n", catalog::describe(&smoke_catalog));
        std::fs::write(&file, redactor.scrub(&(header + &r.transcript)))
            .map_err(|e| e.to_string())?;
        let verdict = if r.failures.is_empty() {
            "PASS"
        } else {
            "FAIL"
        };
        let name = flow.slug();
        println!(
            "{verdict} {name} (acp:{})  trace: {}",
            spec.program,
            file.display()
        );
        for f in &r.failures {
            let kind = match f.kind {
                Kind::Safety => "safety",
                Kind::Capability => "capability",
                Kind::Setup => "setup",
            };
            println!("     [{kind}] {}", redactor.scrub(&f.what));
        }
        for check in &r.not_applicable {
            println!("     [n/a] {check}");
        }
        failed |= !r.failures.is_empty();
        if r.failures.iter().any(|f| f.kind == Kind::Setup) {
            eprintln!(
                "docket-live: the agent did not come up; the remaining flows are not started"
            );
            break;
        }
    }
    redactor.sweep(&args.out);
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

async fn smoke(args: SmokeArgs) -> Result<ExitCode, String> {
    say_reach(args.engine);
    say_agent(&args.agent);
    if let Agent::Acp(spec) = args.agent.clone() {
        return smoke_agent(args, *spec).await;
    }
    let binaries = packaged_binaries().map_err(|e| e.to_string())?;
    let smoke_catalog = args
        .catalog
        .clone()
        .unwrap_or_else(|| catalog::default_source(&args.eval_dir));
    let traces = args.out.join("smoke");
    std::fs::create_dir_all(&traces).map_err(|e| e.to_string())?;
    let patience = Duration::from_secs(args.patience_s);
    let dirs = || (Some(args.out.join("scratch")), Some(smoke_catalog.clone()));
    let keys_home = args
        .keys_home
        .as_deref()
        .map(KeysHome::open)
        .transpose()
        .map_err(|e| e.to_string())?;
    let mut failed = false;
    let plays: Vec<(Play, u32)> = plays(&args)?
        .into_iter()
        .flat_map(|play| {
            let times = if matches!(play, Play::Flow(_)) {
                args.repeat
            } else {
                1
            };
            (1..=times)
                .map(move |run| (play.clone(), run))
                .collect::<Vec<_>>()
        })
        .collect();
    for (play, run) in plays {
        let model: ModelSource = args
            .engine
            .source(play.cassette(), args.inferd_config.as_deref())
            .map_err(|e: EngineError| e.to_string())?;
        let (name, transcript, failures) = match &play {
            Play::Flow(flow) => {
                let r = run_flow(
                    &binaries,
                    *flow,
                    &model,
                    dirs().0,
                    dirs().1,
                    keys_home.as_ref(),
                    patience,
                )
                .await;
                (play.name(), r.transcript, r.failures)
            }
            Play::Hostile(case) => {
                let r = run_planner_case(
                    &binaries,
                    case,
                    &model,
                    dirs(),
                    keys_home.as_ref(),
                    patience,
                )
                .await;
                (play.name(), r.transcript, r.failures)
            }
        };
        let name = match (&play, args.repeat) {
            (Play::Flow(_), n) if n > 1 => format!("{name}.run{run}of{n}"),
            _ => name,
        };
        let file = traces.join(format!("{name}.trace.txt"));
        let header = format!("{}\n\n", catalog::describe(&smoke_catalog));
        std::fs::write(&file, header + &transcript).map_err(|e| e.to_string())?;
        let verdict = if failures.is_empty() { "PASS" } else { "FAIL" };
        println!("{verdict} {name}  trace: {}", file.display());
        for f in &failures {
            let kind = match f.kind {
                Kind::Safety => "safety",
                Kind::Capability => "capability",
                Kind::Setup => "setup",
            };
            println!("     [{kind}] {}", f.what);
        }
        failed |= !failures.is_empty();
        if failures.iter().any(|f| f.kind == Kind::Setup) {
            eprintln!("docket-live: a model did not come up; the remaining flows are not started");
            break;
        }
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
