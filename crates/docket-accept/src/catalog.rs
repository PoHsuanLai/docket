//! The model catalogue a harness world's inferd reads. inferd looks in `$XDG_DATA_HOME/stoker/catalog`
//! (the user's) and `/usr/share/stoker/catalog`; a harness world's XDG_DATA_HOME is scratch, so the
//! entries are copied there from a directory the run names (default: the sibling stoker checkout's
//! `catalog/`). Copied, never linked: nothing in the real tree can be written through the world.

use std::path::{Path, PathBuf};

/// Where the world's inferd finds its user catalogue, under the scratch root.
pub fn world_dir(scratch: &Path) -> PathBuf {
    scratch.join("data/stoker/catalog")
}

/// The default source: `../stoker/catalog` beside the docket checkout whose `eval/` is
/// `eval_dir`, made absolute. Pure: it joins and normalises, it opens nothing.
pub fn default_source(eval_dir: &Path) -> PathBuf {
    let docket = eval_dir.parent().unwrap_or(eval_dir);
    let docket = if docket.as_os_str().is_empty() {
        Path::new(".")
    } else {
        docket
    };
    let base = std::path::absolute(docket).unwrap_or_else(|_| docket.to_path_buf());
    let mut parts: Vec<_> = base.components().collect();
    parts.pop();
    let mut out: PathBuf = parts.iter().collect();
    out.push("stoker");
    out.push("catalog");
    out
}

/// Copies every `*.toml` of `from` into `to` (made if absent); returns how many.
pub fn copy_entries(from: &Path, to: &Path) -> std::io::Result<usize> {
    std::fs::create_dir_all(to)?;
    let mut n = 0;
    for entry in std::fs::read_dir(from)? {
        let path = entry?.path();
        if path.is_file()
            && path.extension().is_some_and(|e| e == "toml")
            && let Some(name) = path.file_name()
        {
            std::fs::copy(&path, to.join(name))?;
            n += 1;
        }
    }
    Ok(n)
}

fn git_sha(dir: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--short=12", "HEAD"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// One line for a header: the directory and, if it is in a git checkout, its commit.
pub fn describe(dir: &Path) -> String {
    match git_sha(dir) {
        Some(sha) => format!("catalogue {} (git {sha})", dir.display()),
        None => format!("catalogue {} (not a git checkout)", dir.display()),
    }
}
