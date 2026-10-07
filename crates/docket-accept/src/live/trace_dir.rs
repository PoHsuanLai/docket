//! The directory a run's traces go in: one transcript per case, the cassette that replays the
//! case's model exchanges, the case itself as a file, and an index. The cassette and the case
//! file are what turn a failure seen live into a deterministic gate test (`eval/regress`, see
//! `docs/live-eval.md`).

use docket_core::ModelExchange;
use docket_eval::{Case, CaseTrace, cassette_from, render_index, trace_file};
use std::path::{Path, PathBuf};

/// Why a trace could not be written.
#[derive(Debug, thiserror::Error)]
pub enum TraceDirError {
    /// The file system said no.
    #[error("{path}: {why}")]
    Io {
        /// Which path.
        path: String,
        /// Why.
        why: String,
    },
    /// The case could not be written as TOML.
    #[error("case {0}: {1}")]
    Toml(String, String),
}

/// A directory of traces.
#[derive(Debug)]
pub struct TraceDir {
    path: PathBuf,
}

fn io(path: &Path, e: &std::io::Error) -> TraceDirError {
    TraceDirError::Io {
        path: path.display().to_string(),
        why: e.to_string(),
    }
}

impl TraceDir {
    /// Makes the directory.
    pub fn create(path: &Path) -> Result<Self, TraceDirError> {
        std::fs::create_dir_all(path).map_err(|e| io(path, &e))?;
        Ok(Self {
            path: path.to_owned(),
        })
    }

    /// Where it is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn put(&self, name: &str, text: &str) -> Result<(), TraceDirError> {
        let file = self.path.join(name);
        std::fs::write(&file, text).map_err(|e| io(&file, &e))
    }

    /// Writes one case's transcript, cassette and case file; returns the transcript's name.
    pub fn write_case(
        &self,
        case: &Case,
        trace: &CaseTrace,
        exchanges: &[ModelExchange],
    ) -> Result<String, TraceDirError> {
        let name = trace_file(&case.id);
        self.put(&name, &trace.render())?;
        self.put(
            &format!("{}.cassette.jsonl", case.id.0),
            &cassette_from(exchanges, "docket-live"),
        )?;
        let toml = toml::to_string_pretty(case)
            .map_err(|e| TraceDirError::Toml(case.id.0.clone(), e.to_string()))?;
        self.put(&format!("{}.case.toml", case.id.0), &toml)?;
        Ok(name)
    }

    /// Writes `index.txt`.
    pub fn write_index(
        &self,
        header: &str,
        traces: &[(String, CaseTrace)],
    ) -> Result<(), TraceDirError> {
        let rows: Vec<(String, &CaseTrace)> =
            traces.iter().map(|(file, t)| (file.clone(), t)).collect();
        self.put("index.txt", &format!("{header}\n{}", render_index(&rows)))
    }
}
