//! `docket-eval --check-app <dir>`: the conformance check of cli.md section 6 over one app's
//! repository. Exit 0 when the app conforms, 1 when it does not, 2 when the command line or the
//! directory is wrong.

use docket_eval::check_app;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: docket-eval --check-app <repository dir>\n";

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let [flag, dir] = words.as_slice() else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    if flag != "--check-app" {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    }
    match check_app(&PathBuf::from(dir)) {
        Ok(report) => {
            print!("{}", report.render());
            match report.failures() {
                0 => ExitCode::SUCCESS,
                n => {
                    eprintln!(
                        "docket-eval: {n} failure(s): this app does not conform to cli.md section 6"
                    );
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
