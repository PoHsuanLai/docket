//! `docket-eval --check-app <dir>`: the conformance check of cli.md section 6 over one app's
//! repository; `docket-eval --check-skills <dir>… [--manifests <dir>]…`: every skill under the
//! directories validates and uses only registered actions, found in the manifests under the
//! directories (as `--check-app` finds them, plus `dist/intents/*.toml` of each, and the
//! `intents` directory beside a `skills` directory) or in a `--manifests` directory. Exit 0 when it conforms, 1 when it does not, 2 when the
//! command line or a directory is wrong.

use docket_eval::{CheckError, CheckReport, check_app, check_skills};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: docket-eval --check-app <repository dir>\n       docket-eval --check-skills <dir>... [--manifests <dir>]...\n";

/// The directories to check and the extra manifest directories, from what follows
/// `--check-skills`. `None` when no directory is named or `--manifests` has no value.
fn skill_args(words: &[String]) -> Option<(Vec<PathBuf>, Vec<PathBuf>)> {
    let mut dirs = Vec::new();
    let mut manifests = Vec::new();
    let mut words = words.iter();
    while let Some(word) = words.next() {
        if word == "--manifests" {
            manifests.push(PathBuf::from(words.next()?));
        } else {
            dirs.push(PathBuf::from(word));
        }
    }
    (!dirs.is_empty()).then_some((dirs, manifests))
}

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let (what, checked) = match words.split_first() {
        Some((flag, [dir])) if flag == "--check-app" => {
            ("cli.md section 6", check_app(&PathBuf::from(dir)))
        }
        Some((flag, rest)) if flag == "--check-skills" => match skill_args(rest) {
            Some((dirs, manifests)) => ("the skills format", check_skills(&dirs, &manifests)),
            None => {
                eprint!("{USAGE}");
                return ExitCode::from(2);
            }
        },
        _ => {
            eprint!("{USAGE}");
            return ExitCode::from(2);
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
