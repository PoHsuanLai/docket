//! `docket-eval --check-app <dir>`: the conformance check of cli.md section 6 over one app's
//! repository; `docket-eval --check-skills <dir>… [--manifests <dir>]…`: every skill under the
//! directories validates and uses only registered actions, found in the manifests under the
//! directories (as `--check-app` finds them, plus `dist/intents/*.toml` of each, and the
//! `intents` directory beside a `skills` directory) or in a `--manifests` directory. Exit 0 when it conforms, 1 when it does not, 2 when the
//! command line or a directory is wrong.

mod attack_cli;
mod cli;

use clap::Parser;
use cli::{Cli, Request};
use docket_eval::{CheckError, CheckReport, check_app, check_skills};
use eval_core::utc_date;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

/// Today in UTC, as `YYYY-MM-DD`: the one clock read of the attack summary.
fn today() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    utc_date(secs)
}

fn main() -> ExitCode {
    let Some(request) = Cli::parse().request() else {
        return ExitCode::from(2);
    };
    let (what, checked) = match request {
        Request::Attacks(args) => return attack_cli::attacks(&args, &today()),
        Request::AttackDiff { before, after } => return attack_cli::diff(&before, &after),
        Request::CheckApp(dir) => ("cli.md section 6", check_app(&dir)),
        Request::CheckSkills { dirs, manifests } => {
            ("the skills format", check_skills(&dirs, &manifests))
        }
    };
    finish(what, checked)
}

fn finish(what: &str, checked: Result<CheckReport, CheckError>) -> ExitCode {
    match checked {
        Ok(report) => {
            print!("{}", report.render());
            match report.failures() {
                0 => ExitCode::SUCCESS,
                n => {
                    eprintln!("docket-eval: {n} failure(s): this does not conform to {what}");
                    ExitCode::from(1)
                }
            }
        }
        Err(why) => {
            eprintln!("docket-eval: {why}");
            ExitCode::from(2)
        }
    }
}
