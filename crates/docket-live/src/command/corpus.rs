//! `docket-live corpus`.

use super::{CommandFault, say_agent, say_reach};
use crate::acp::AcpSpec;
use crate::attack_planner::run_attacks_planner;
use crate::cli::{Agent, CorpusArgs, PlannerMode};
use crate::report_file::write_report;
use crate::{CorpusOptions, CorpusOutcome, Engine, Timeouts, regress, run_corpus_live};
use docket_accept::catalog;
use docket_accept::keys_home::KeysHome;
use docket_accept::world::Binaries;
use docket_eval::{load_all, load_attacks, stamp_summary, write_summary};
use docket_vocab::Millis;
use eval_core::{
    AttackCase, AttackMetrics, Case, Corpus, LiveMetrics, RunNote, RunReport, planner_slug,
    score_traces, utc_date,
};
use std::process::ExitCode;
use std::time::Duration;

const LIVE_BOUND_MS: u32 = 30_000;

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
fn chosen_attacks(args: &CorpusArgs) -> Result<Vec<AttackCase>, CommandFault> {
    let named = args.corpora.iter().any(|c| c == ATTACKS);
    if !named && args.cases.is_empty() {
        return Ok(Vec::new());
    }
    let all = load_attacks(&args.eval_dir.join("injection").join(ATTACKS))?;
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
fn corpus_agent(args: &CorpusArgs, spec: &AcpSpec) -> Result<ExitCode, CommandFault> {
    let all = load_all(&args.eval_dir)?;
    let cases = chosen(args, all);
    if cases.is_empty() {
        return Err(CommandFault::NoCase);
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
    outcomes: &[CorpusOutcome],
    dir: &std::path::Path,
) -> Result<(), CommandFault> {
    let Some(metrics) = outcomes.iter().find_map(|o| o.report.attack.clone()) else {
        return Err(CommandFault::NoAttackSummary);
    };
    let summary = stamp_summary(args.engine.slug(), metrics, &args.eval_dir, &today())?;
    let path = write_summary(&summary, dir)?;
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
) -> Result<ExitCode, CommandFault> {
    if args.regress.is_some() || args.corpora.iter().any(|c| c != ATTACKS) {
        return Err(CommandFault::LivePlannerOnlyAttacks);
    }
    let attacks = chosen_attacks(args)?;
    if attacks.is_empty() {
        return Err(CommandFault::NoAttacks);
    }
    eprintln!(
        "docket-live: --planner live: {} attacks, two runs each, played by the companion's own planner model (a run takes a while)",
        attacks.len()
    );
    let ran = run_attacks_planner(binaries, &attacks, options).await?;
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
        let summary = stamp_summary(&engine, metrics.base.clone(), &args.eval_dir, &today())?;
        let path = write_summary(&summary, dir)?;
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

pub(super) async fn corpus(
    args: CorpusArgs,
    binaries: &Binaries,
) -> Result<ExitCode, CommandFault> {
    say_reach(args.engine);
    say_agent(&args.agent);
    if let Agent::Acp(spec) = &args.agent {
        return corpus_agent(&args, spec);
    }
    let keys_home = args.keys_home.as_deref().map(KeysHome::open).transpose()?;
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
        return corpus_live_planner(&args, binaries, &options).await;
    }
    let mut misses = Vec::new();
    let mut outcomes = Vec::new();
    let all = load_all(&args.eval_dir)?;
    match &args.regress {
        Some(dir) => {
            for pair in regress::load(dir, &all)? {
                options.cassette = Some(pair.cassette.clone());
                options.out = args.out.join(&pair.case.id.0);
                let ran =
                    run_corpus_live(binaries, std::slice::from_ref(&pair.case), &options).await?;
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
                return Err(CommandFault::NoCase);
            }
            let mut ran = run_corpus_live(binaries, &cases, &options).await?;
            if !attacks.is_empty() {
                let scores = score_traces(&attacks, &ran.traces)?;
                ran.report.attack = Some(AttackMetrics::from_scores(&scores));
            }
            misses.extend(ran.note.missed.clone());
            outcomes.push(ran);
        }
    }
    if outcomes.is_empty() {
        return Err(CommandFault::NothingToRun);
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
