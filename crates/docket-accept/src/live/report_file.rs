//! Writing the run's report: the directory it goes in is made if it is not there, and a failure
//! names the path it was about.

use std::path::Path;

/// Writes `text` to `path`, making the parent directory first.
pub fn write_report(path: &Path, text: &str) -> Result<(), String> {
    let fail = |at: &Path, e: std::io::Error| format!("{}: {e}", at.display());
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| fail(dir, e))?;
    }
    std::fs::write(path, text).map_err(|e| fail(path, e))
}
