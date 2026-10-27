//! The attack summary on disk: stamped with a commit and a day, written under a directory and
//! read back. The day comes in as a parameter; this crate reads no clock.

use eval_core::{AttackMetrics, AttackSummary, SummaryError};
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// The numbers of `metrics`, stamped with `date` (`YYYY-MM-DD`), the commit `repo` is at and
/// the `engine` that answered.
pub fn stamp_summary(
    engine: &str,
    metrics: AttackMetrics,
    repo: &Path,
    date: &str,
) -> Result<AttackSummary, SummaryError> {
    Ok(AttackSummary {
        date: date.to_owned(),
        commit: short_commit(repo)?,
        engine: engine.to_owned(),
        metrics,
    })
}

/// Writes the summary into `dir` (made if it is not there) and returns the path.
pub fn write_summary(summary: &AttackSummary, dir: &Path) -> Result<PathBuf, SummaryError> {
    let io = |path: &Path, e: std::io::Error| SummaryError::Io {
        path: path.to_owned(),
        why: e.to_string(),
    };
    let path = dir.join(summary.file_name()?);
    std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    std::fs::write(&path, summary.to_json(&path)?).map_err(|e| io(&path, e))?;
    Ok(path)
}

/// Reads a summary back.
pub fn read_summary(path: &Path) -> Result<AttackSummary, SummaryError> {
    let text = std::fs::read_to_string(path).map_err(|e| SummaryError::Io {
        path: path.to_owned(),
        why: e.to_string(),
    })?;
    AttackSummary::from_json(path, &text)
}
