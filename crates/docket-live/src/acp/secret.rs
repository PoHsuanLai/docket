//! The login of a `login` agent, for the length of one run.
//!
//! The harness takes a file from the flag `--acp-credentials` and copies it into the scratch HOME,
//! at the agent's expected relative path, mode 0600. It reads nothing else from anywhere and
//! writes nothing outside the scratch root. `Credentials` removes the copy when it goes out of
//! scope, so a failure or a panic removes it too. Its content is also kept in memory as a
//! `Redactor`, which scrubs it from every trace, report and log the harness writes and from the
//! scratch tree at the end of the run, whatever an agent has said or done with it.

use super::spec::CredentialsSource;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

/// The text that replaces a secret.
pub const REDACTED: &str = "[redacted]";

/// The shortest string worth scrubbing: shorter ones are words (`max`, `user`), not tokens.
const SHORTEST: usize = 8;

/// Why the credentials could not be staged.
#[derive(Debug, thiserror::Error)]
pub enum StageFault {
    /// The source did not read.
    #[error("--acp-credentials: {0}")]
    Read(String),
    /// The copy could not be made in the scratch HOME.
    #[error("the scratch HOME: {0}")]
    Write(String),
}

/// The copy in the scratch HOME. Dropping it removes the file.
#[derive(Debug)]
pub struct Credentials {
    at: PathBuf,
}

impl Credentials {
    /// Copies `source.from` to `home/<source.at>` with mode 0600 and returns the guard and what
    /// to scrub. The content is never printed: the errors name paths, not bytes.
    pub fn stage(source: &CredentialsSource, home: &Path) -> Result<(Self, Redactor), StageFault> {
        let bytes = std::fs::read(&source.from).map_err(|e| StageFault::Read(e.to_string()))?;
        let at = home.join(&source.at);
        let write = |e: std::io::Error| StageFault::Write(e.to_string());
        if let Some(dir) = at.parent() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)
                .map_err(write)?;
        }
        // Replace anything an earlier run left rather than write through it.
        let _ = std::fs::remove_file(&at);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&at)
            .map_err(write)?;
        // The guard exists from here, so a failed write still removes the file.
        let guard = Self { at };
        file.write_all(&bytes).map_err(write)?;
        Ok((guard, Redactor::of(&bytes)))
    }

    /// Where the copy is.
    pub fn path(&self) -> &Path {
        &self.at
    }
}

impl Drop for Credentials {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.at);
    }
}

/// What to scrub: the whole text of a secret file and every string inside it that is long enough
/// to be a token.
#[derive(Clone, Default)]
pub struct Redactor {
    needles: Vec<String>,
}

impl std::fmt::Debug for Redactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Redactor({} needles)", self.needles.len())
    }
}

fn strings(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => out.push(text.clone()),
        serde_json::Value::Array(items) => items.iter().for_each(|v| strings(v, out)),
        serde_json::Value::Object(map) => map.values().for_each(|v| strings(v, out)),
        _ => {}
    }
}

impl Redactor {
    /// Scrubs nothing: a run with no credentials.
    pub fn none() -> Self {
        Self::default()
    }

    /// Scrubs the content of a secret file given as `bytes`.
    pub fn of(bytes: &[u8]) -> Self {
        let text = String::from_utf8_lossy(bytes).trim().to_owned();
        let mut needles = vec![text.clone()];
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
            strings(&json, &mut needles);
        }
        needles.retain(|n| n.len() >= SHORTEST);
        needles.sort_by_key(|n| std::cmp::Reverse(n.len()));
        needles.dedup();
        Self { needles }
    }

    /// `text` with every secret replaced.
    pub fn scrub(&self, text: &str) -> String {
        self.needles
            .iter()
            .fold(text.to_owned(), |t, n| t.replace(n.as_str(), REDACTED))
    }

    /// Whether `bytes` hold none of the secrets.
    pub fn is_clean(&self, bytes: &[u8]) -> bool {
        let text = String::from_utf8_lossy(bytes);
        self.needles.iter().all(|n| !text.contains(n.as_str()))
    }

    /// Rewrites every file under `root` that holds a secret, with the secrets replaced. Files that
    /// are large, unreadable or not regular are left; what the agent keeps in its own state
    /// directory is the agent's, but the files the harness and the daemons write are all here.
    pub fn sweep(&self, root: &Path) {
        if self.needles.is_empty() {
            return;
        }
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => self.sweep(&path),
                Ok(kind) if kind.is_file() => self.sweep_file(&path),
                _ => {}
            }
        }
    }

    fn sweep_file(&self, path: &Path) {
        const LARGEST: u64 = 64 * 1024 * 1024;
        if !std::fs::metadata(path).is_ok_and(|m| m.len() <= LARGEST) {
            return;
        }
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        if !self.is_clean(&bytes) {
            let clean = self.scrub(&String::from_utf8_lossy(&bytes));
            let _ = std::fs::write(path, clean);
        }
    }
}
