//! Writing the run's report: the directory it goes in is made if it is not there, and a failure
//! names the path it was about.

use std::path::{Path, PathBuf};

/// Why a report could not be written: the path it was about, and the reason.
#[derive(Debug, thiserror::Error)]
#[error("{}: {source}", path.display())]
pub struct ReportFault {
    path: PathBuf,
    source: std::io::Error,
}

impl ReportFault {
    fn at(path: &Path) -> impl FnOnce(std::io::Error) -> Self + '_ {
        move |source| Self {
            path: path.to_owned(),
            source,
        }
    }
}

/// Writes `text` to `path`, making the parent directory first.
pub fn write_report(path: &Path, text: &str) -> Result<(), ReportFault> {
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(ReportFault::at(dir))?;
    }
    std::fs::write(path, text).map_err(ReportFault::at(path))
}
