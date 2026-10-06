//! `docket-eval --check-app <dir>`: the conformance check of cli.md section 6 over one app's
//! repository; `docket-eval --check-skills <dir>…`: every skill under the directories validates
//! and uses only registered actions. Exit 0 when it conforms, 1 when it does not, 2 when the
//! command line or a directory is wrong.

use docket_eval::{CheckError, CheckReport, check_app, check_skills};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: docket-eval --check-app <repository dir>\n       docket-eval --check-skills <dir>...\n";

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let (what, checked) = match words.split_first() {
        Some((flag, [dir])) if flag == "--check-app" => {
            ("cli.md section 6", check_app(&PathBuf::from(dir)))
        }
        Some((flag, dirs)) if flag == "--check-skills" && !dirs.is_empty() => {
            let dirs: Vec<PathBuf> = dirs.iter().map(PathBuf::from).collect();
            ("the skills format", check_skills(&dirs))
        }
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
