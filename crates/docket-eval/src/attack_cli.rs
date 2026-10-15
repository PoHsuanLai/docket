//! `docket-eval --attacks <eval dir> [--summary-dir <dir>] [--commit <sha>] [--date <day>]`: plays
//! the attack suite on the scripted, hostile fake model (no network, no model) and prints
//! AgentDojo's three rates. With `--summary-dir` it also writes the per-commit JSON summary
//! there (`<date>-<commit>.json`); the commit and the day are the checkout's and today's unless
//! given. Exit 0 when no attack reaches its goal beyond the known holes, 1 when one does, 2 when
//! the command line or a file is wrong.
//!
//! `docket-eval --attack-diff <before.json> <after.json>`: compares two summaries and exits 1
//! when the attack success rate rose (or an attack reaches its goal that did not before).

use docket_core::AgentConfig;
use docket_eval::{
    AttackMetrics, AttackSummary, Comparison, FAKE_ENGINE, Harness, SummaryError, Verdict,
    load_attacks, run_attacks, short_commit, today,
};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// What `--attacks` is told.
#[derive(Debug, Default)]
struct Args {
    eval_dir: PathBuf,
    summary_dir: Option<PathBuf>,
    commit: Option<String>,
    date: Option<String>,
}

fn parse(words: &[String]) -> Option<Args> {
    let (dir, rest) = words.split_first()?;
    let mut args = Args {
        eval_dir: PathBuf::from(dir),
        ..Args::default()
    };
    let mut rest = rest.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next()?;
        match flag.as_str() {
            "--summary-dir" => args.summary_dir = Some(PathBuf::from(value)),
            "--commit" => args.commit = Some(value.clone()),
            "--date" => args.date = Some(value.clone()),
            _ => return None,
        }
    }
    Some(args)
}

fn fail(why: impl std::fmt::Display) -> ExitCode {
    eprintln!("docket-eval: {why}");
    ExitCode::from(2)
}

fn stamped(args: &Args, metrics: AttackMetrics) -> Result<AttackSummary, SummaryError> {
    let commit = match &args.commit {
        Some(commit) => commit.clone(),
        None => short_commit(&args.eval_dir)?,
    };
    Ok(AttackSummary {
        date: args.date.clone().unwrap_or_else(today),
        commit,
        engine: FAKE_ENGINE.to_owned(),
        metrics,
    })
}

/// `--attacks`.
pub(crate) fn attacks(words: &[String], usage: &str) -> ExitCode {
    let Some(args) = parse(words) else {
        eprint!("{usage}");
        return ExitCode::from(2);
    };
    let dir = args.eval_dir.join("injection").join("attacks");
    let attacks = match load_attacks(&dir) {
        Ok(attacks) if !attacks.is_empty() => attacks,
        Ok(_) => return fail(format!("no attacks in {}", dir.display())),
        Err(why) => return fail(why),
    };
    let harness = match Harness::new(AgentConfig::default()) {
        Ok(harness) => harness,
        Err(why) => return fail(format!("the fake router: {why}")),
    };
    let scores = match run_attacks(&attacks, &harness) {
        Ok(scores) => scores,
        Err(why) => return fail(why),
    };
    let metrics = AttackMetrics::from_scores(&scores);
    print!("{}", metrics.render());
    if let Some(summary_dir) = &args.summary_dir {
        let written = stamped(&args, metrics.clone()).and_then(|s| s.write_to(summary_dir));
        match written {
            Ok(path) => eprintln!("docket-eval: summary written to {}", path.display()),
            Err(why) => return fail(why),
        }
    }
    match metrics.surprises().is_empty() {
        true => ExitCode::SUCCESS,
        false => {
            eprintln!(
                "docket-eval: attacks reached their goal: {:?}",
                metrics.surprises()
            );
            ExitCode::from(1)
        }
    }
}

/// `--attack-diff`.
pub(crate) fn diff(before: &Path, after: &Path) -> ExitCode {
    let read = |path: &Path| AttackSummary::read(path);
    let (before, after) = match (read(before), read(after)) {
        (Ok(before), Ok(after)) => (before, after),
        (Err(why), _) | (_, Err(why)) => return fail(why),
    };
    let comparison = Comparison::of(before, after);
    print!("{}", comparison.render());
    match comparison.verdict() {
        Verdict::Held => ExitCode::SUCCESS,
        Verdict::Rose => ExitCode::from(1),
    }
}
