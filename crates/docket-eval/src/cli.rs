//! The command line of `docket-eval`: one mode flag and what it needs. clap reads it; a command
//! line it refuses ends the program with exit 2.

use clap::{ArgGroup, Parser};
use std::path::PathBuf;

/// What `--attacks` is told.
#[derive(Debug)]
pub(crate) struct AttackArgs {
    pub(crate) eval_dir: PathBuf,
    pub(crate) summary_dir: Option<PathBuf>,
    pub(crate) commit: Option<String>,
    pub(crate) date: Option<String>,
}

/// What was asked for.
#[derive(Debug)]
pub(crate) enum Request {
    /// `--check-app <repository dir>`.
    CheckApp(PathBuf),
    /// `--check-skills <dir>... [--manifests <dir>]...`.
    CheckSkills {
        dirs: Vec<PathBuf>,
        manifests: Vec<PathBuf>,
    },
    /// `--attacks <eval dir> [--summary-dir <dir>] [--commit <sha>] [--date <day>]`.
    Attacks(AttackArgs),
    /// `--attack-diff <before.json> <after.json>`.
    AttackDiff { before: PathBuf, after: PathBuf },
}

/// The command line as clap reads it.
#[derive(Debug, Parser)]
#[command(
    name = "docket-eval",
    about = "The conformance checks and the attack suite",
    group = ArgGroup::new("mode")
        .required(true)
        .multiple(false)
        .args(["check_app", "check_skills", "attacks", "attack_diff"])
)]
pub(crate) struct Cli {
    /// The conformance check of cli.md section 6 over one app's repository.
    #[arg(long, value_name = "REPOSITORY_DIR")]
    check_app: Option<PathBuf>,
    /// Every skill under these directories validates and uses only registered actions.
    #[arg(long, value_name = "DIR", num_args = 1..)]
    check_skills: Option<Vec<PathBuf>>,
    /// A directory of manifests besides the ones found under the skills' directories.
    #[arg(long, value_name = "DIR", requires = "check_skills")]
    manifests: Vec<PathBuf>,
    /// Play the attack suite on the scripted, hostile fake model.
    #[arg(long, value_name = "EVAL_DIR")]
    attacks: Option<PathBuf>,
    /// Also write the per-commit JSON summary here.
    #[arg(long, value_name = "DIR", requires = "attacks")]
    summary_dir: Option<PathBuf>,
    /// The commit the summary names (the checkout's unless given).
    #[arg(long, value_name = "SHA", requires = "attacks")]
    commit: Option<String>,
    /// The day the summary names (today unless given).
    #[arg(long, value_name = "DAY", requires = "attacks")]
    date: Option<String>,
    /// Compare two summaries; exit 1 when the attack success rate rose.
    #[arg(long, value_names = ["BEFORE_JSON", "AFTER_JSON"], num_args = 2)]
    attack_diff: Option<Vec<PathBuf>>,
}

impl Cli {
    /// The request the command line makes. The mode group holds that exactly one is given.
    pub(crate) fn request(self) -> Option<Request> {
        if let Some(dir) = self.check_app {
            return Some(Request::CheckApp(dir));
        }
        if let Some(dirs) = self.check_skills {
            return Some(Request::CheckSkills {
                dirs,
                manifests: self.manifests,
            });
        }
        if let Some(eval_dir) = self.attacks {
            return Some(Request::Attacks(AttackArgs {
                eval_dir,
                summary_dir: self.summary_dir,
                commit: self.commit,
                date: self.date,
            }));
        }
        match self.attack_diff?.as_slice() {
            [before, after] => Some(Request::AttackDiff {
                before: before.clone(),
                after: after.clone(),
            }),
            _ => None,
        }
    }
}
