//! Reading the corpus: every `*.toml` of a directory is one case.

use crate::case::{Case, CaseId};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Why the corpus could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CorpusError {
    /// A directory or file could not be read.
    #[error("{path}: {why}")]
    Io {
        /// Where.
        path: PathBuf,
        /// Why.
        why: String,
    },
    /// A file is not a case.
    #[error("{path}: {why}")]
    Parse {
        /// Where.
        path: PathBuf,
        /// Why.
        why: String,
    },
    /// Two cases share an id.
    #[error("case id {0:?} is used twice")]
    DuplicateId(CaseId),
    /// A case has no `why`, or no steps.
    #[error("{path}: a case needs a why and at least one planner step")]
    Incomplete {
        /// Where.
        path: PathBuf,
    },
}

fn io(path: &Path, e: &std::io::Error) -> CorpusError {
    CorpusError::Io {
        path: path.to_owned(),
        why: e.to_string(),
    }
}

/// The paths inside `dir` with the given extension (or, for `None`, the directories), sorted.
fn sorted(dir: &Path, extension: Option<&str>) -> Result<Vec<PathBuf>, CorpusError> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| io(dir, &e))? {
        let path = entry.map_err(|e| io(dir, &e))?.path();
        let keep = match extension {
            Some(ext) => path.is_file() && path.extension().is_some_and(|e| e == ext),
            None => path.is_dir(),
        };
        if keep {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

/// Reads every case of one directory, in file-name order.
pub fn load_corpus(dir: &Path) -> Result<Vec<Case>, CorpusError> {
    sorted(dir, Some("toml"))?
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path).map_err(|e| io(&path, &e))?;
            let case: Case = toml::from_str(&text).map_err(|e| CorpusError::Parse {
                path: path.clone(),
                why: e.to_string(),
            })?;
            if case.why.trim().is_empty() || case.planner.is_empty() {
                return Err(CorpusError::Incomplete { path });
            }
            Ok(case)
        })
        .collect()
}

/// Reads every subdirectory of `root` and checks that case ids are unique across all of them.
pub fn load_all(root: &Path) -> Result<Vec<Case>, CorpusError> {
    let mut all = Vec::new();
    let mut seen = BTreeSet::new();
    for dir in sorted(root, None)? {
        for case in load_corpus(&dir)? {
            if !seen.insert(case.id.clone()) {
                return Err(CorpusError::DuplicateId(case.id));
            }
            all.push(case);
        }
    }
    Ok(all)
}
