//! A report per commit: the attack suite's numbers as a small JSON file under `eval/results/`,
//! named by date and short commit, and a comparison of two of them that flags a rise in the
//! attack success rate.
//!
//! The file keeps the three rates with their intervals and the ids behind the attack success
//! rate, so a later run can say which attack got through that did not before. A run on the
//! hostile fake model (the gate's) is `<date>-<commit>.json`; a run over a real model adds its
//! engine: `<date>-<commit>-<engine>.json`.

use crate::attack_score::AttackMetrics;
use crate::report::Rate95;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// The engine of the gate's own run: the scripted planner and reviewers that allow everything.
pub const FAKE_ENGINE: &str = "hostile-fake";

/// Why a summary could not be written, read or named.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SummaryError {
    /// A file or directory could not be read or written.
    #[error("{path}: {why}")]
    Io {
        /// Where.
        path: PathBuf,
        /// Why.
        why: String,
    },
    /// A file is not a summary.
    #[error("{path}: {why}")]
    Parse {
        /// Where.
        path: PathBuf,
        /// Why.
        why: String,
    },
    /// The date, commit or engine would make a file name that is not plain.
    #[error("{0:?} is not a plain name part (letters, digits, '.', '_' and '-')")]
    Name(String),
    /// The commit could not be found.
    #[error("no commit for {path}: {why}")]
    Commit {
        /// The checkout asked.
        path: PathBuf,
        /// Why.
        why: String,
    },
}

/// One run's numbers, stamped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttackSummary {
    /// The UTC day of the run, `YYYY-MM-DD`.
    pub date: String,
    /// The short commit measured (with `-dirty` when the checkout had changes).
    pub commit: String,
    /// What answered: [`FAKE_ENGINE`], `scripted`, `local` or `cloud`.
    pub engine: String,
    /// The numbers.
    pub metrics: AttackMetrics,
}

fn plain(part: &str) -> Result<(), SummaryError> {
    let ok = !part.is_empty()
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    match ok {
        true => Ok(()),
        false => Err(SummaryError::Name(part.to_owned())),
    }
}

/// The UTC calendar day of a Unix time, as `YYYY-MM-DD`.
pub fn utc_date(unix_secs: u64) -> String {
    let z = i64::try_from(unix_secs / 86_400).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Today in UTC.
pub fn today() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    utc_date(secs)
}

fn git(repo: &Path, args: &[&str]) -> Result<String, SummaryError> {
    let fault = |why: String| SummaryError::Commit {
        path: repo.to_owned(),
        why,
    };
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| fault(e.to_string()))?;
    match out.status.success() {
        true => Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned()),
        false => Err(fault(
            String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        )),
    }
}

/// The short commit of the checkout `repo` is in, with `-dirty` when tracked files have changes.
pub fn short_commit(repo: &Path) -> Result<String, SummaryError> {
    let sha = git(repo, &["rev-parse", "--short", "HEAD"])?;
    let changes = git(repo, &["status", "--porcelain", "--untracked-files=no"])?;
    Ok(match changes.is_empty() {
        true => sha,
        false => format!("{sha}-dirty"),
    })
}

impl AttackSummary {
    /// The numbers of `metrics`, stamped with today and the commit `repo` is at.
    pub fn stamp(engine: &str, metrics: AttackMetrics, repo: &Path) -> Result<Self, SummaryError> {
        Ok(Self {
            date: today(),
            commit: short_commit(repo)?,
            engine: engine.to_owned(),
            metrics,
        })
    }

    /// The file this summary is kept in.
    pub fn file_name(&self) -> Result<String, SummaryError> {
        [&self.date, &self.commit, &self.engine]
            .into_iter()
            .try_for_each(|part| plain(part))?;
        Ok(match self.engine == FAKE_ENGINE {
            true => format!("{}-{}.json", self.date, self.commit),
            false => format!("{}-{}-{}.json", self.date, self.commit, self.engine),
        })
    }

    /// Writes the summary into `dir` (made if it is not there) and returns the path.
    pub fn write_to(&self, dir: &Path) -> Result<PathBuf, SummaryError> {
        let io = |path: &Path, e: std::io::Error| SummaryError::Io {
            path: path.to_owned(),
            why: e.to_string(),
        };
        let path = dir.join(self.file_name()?);
        std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
        let mut text = serde_json::to_string_pretty(self).map_err(|e| SummaryError::Parse {
            path: path.clone(),
            why: e.to_string(),
        })?;
        text.push('\n');
        std::fs::write(&path, text).map_err(|e| io(&path, e))?;
        Ok(path)
    }

    /// Reads a summary back.
    pub fn read(path: &Path) -> Result<Self, SummaryError> {
        let text = std::fs::read_to_string(path).map_err(|e| SummaryError::Io {
            path: path.to_owned(),
            why: e.to_string(),
        })?;
        serde_json::from_str(&text).map_err(|e| SummaryError::Parse {
            path: path.to_owned(),
            why: e.to_string(),
        })
    }

    fn label(&self) -> String {
        format!("{} {} ({})", self.date, self.commit, self.engine)
    }
}

/// Whether the attack success rate went up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// It did not rise, and no attack reaches its goal that did not before.
    Held,
    /// The rate rose, or an attack reaches its goal that did not before.
    Rose,
}

/// Whether a rise is more than the runs' noise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Significance {
    /// The new rate's interval lies wholly above the old one's.
    Clear,
    /// The intervals overlap: with this many attacks the rise could be chance.
    WithinNoise,
}

/// Two summaries side by side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparison {
    /// The earlier run.
    pub before: AttackSummary,
    /// The later run.
    pub after: AttackSummary,
    /// Attacks that reach their goal in the later run and did not in the earlier.
    pub newly_reached: Vec<String>,
    /// Attacks that reached their goal in the earlier run and no longer do.
    pub no_longer_reached: Vec<String>,
}

fn only_in(this: &[String], other: &[String]) -> Vec<String> {
    this.iter()
        .filter(|id| !other.contains(id))
        .cloned()
        .collect()
}

impl Comparison {
    /// Compares `before` with `after`.
    pub fn of(before: AttackSummary, after: AttackSummary) -> Self {
        Self {
            newly_reached: only_in(&after.metrics.reached, &before.metrics.reached),
            no_longer_reached: only_in(&before.metrics.reached, &after.metrics.reached),
            before,
            after,
        }
    }

    /// Whether the attack success rate rose.
    pub fn verdict(&self) -> Verdict {
        let rose = self.after.metrics.asr.point > self.before.metrics.asr.point;
        match rose || !self.newly_reached.is_empty() {
            true => Verdict::Rose,
            false => Verdict::Held,
        }
    }

    /// Whether a rise stands out from the noise.
    pub fn significance(&self) -> Significance {
        match self.after.metrics.asr.low > self.before.metrics.asr.high {
            true => Significance::Clear,
            false => Significance::WithinNoise,
        }
    }

    /// The comparison as lines for a terminal.
    pub fn render(&self) -> String {
        let pct = |p: u32| format!("{}.{}%", p / 10, p % 10);
        let rate =
            |r: &Rate95| format!("{} [{} - {}]", pct(r.point.0), pct(r.low.0), pct(r.high.0));
        let (b, a) = (&self.before.metrics, &self.after.metrics);
        let mut out = format!(
            "attack suite: {} ({} attacks) -> {} ({} attacks)\n",
            self.before.label(),
            b.n.0,
            self.after.label(),
            a.n.0
        );
        out.push_str(&format!(
            "  utility without attack:  {} -> {}\n",
            rate(&b.utility),
            rate(&a.utility)
        ));
        out.push_str(&format!(
            "  utility under attack:    {} -> {}\n",
            rate(&b.utility_under_attack),
            rate(&a.utility_under_attack)
        ));
        let word = match (self.verdict(), self.significance()) {
            (Verdict::Held, _) => "held",
            (Verdict::Rose, Significance::Clear) => "ROSE (clearly: the intervals do not overlap)",
            (Verdict::Rose, Significance::WithinNoise) => {
                "ROSE (within noise at this many attacks)"
            }
        };
        out.push_str(&format!(
            "  attack success rate:     {} -> {}  {word}\n",
            rate(&b.asr),
            rate(&a.asr)
        ));
        for (label, ids) in [
            ("newly reach their goal", &self.newly_reached),
            ("no longer reach their goal", &self.no_longer_reached),
        ] {
            if !ids.is_empty() {
                out.push_str(&format!("  {label}: {}\n", ids.join(", ")));
            }
        }
        out
    }
}
