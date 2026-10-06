//! Live to regression: the cassette of a live run that went wrong, kept beside the corpus as
//! `eval/regress/<case id>.cassette.jsonl`. The gate replays each one with the model answering
//! exactly as it did live, through the same case, and requires the case's expectation to hold:
//! a failure seen against a real model is fixed once and stays fixed. The case itself is an
//! ordinary corpus case (`eval/<corpus>/<id>.toml`); a scenario seen only in a companion run is
//! written up as a new case first (the run's `<id>.case.toml` is the place to start).

use docket_eval::Case;
use std::path::Path;

/// One case with the cassette of the live run that went wrong.
#[derive(Debug, Clone)]
pub struct Regression {
    /// The case.
    pub case: Case,
    /// The cassette its live run made.
    pub cassette: String,
}

/// Why the regressions could not be read.
#[derive(Debug, thiserror::Error)]
pub enum RegressError {
    /// A directory or file could not be read.
    #[error("{path}: {why}")]
    Io {
        /// Where.
        path: String,
        /// Why.
        why: String,
    },
    /// A cassette names no case of the corpus.
    #[error("{0}.cassette.jsonl names no case: no case has the id {0:?}")]
    NoCase(String),
}

/// Every cassette in `dir`, paired with the case of the same id among `cases`. None when the
/// directory is not there.
pub fn load(dir: &Path, cases: &[Case]) -> Result<Vec<Regression>, RegressError> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let io = |path: &Path, e: &std::io::Error| RegressError::Io {
        path: path.display().to_string(),
        why: e.to_string(),
    };
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| io(dir, &e))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| p.to_string_lossy().ends_with(".cassette.jsonl"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|file| {
            let name = file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let id = name.trim_end_matches(".cassette.jsonl").to_owned();
            let case = cases
                .iter()
                .find(|c| c.id.0 == id)
                .ok_or_else(|| RegressError::NoCase(id.clone()))?;
            let cassette = std::fs::read_to_string(&file).map_err(|e| io(&file, &e))?;
            Ok(Regression {
                case: case.clone(),
                cassette,
            })
        })
        .collect()
}
