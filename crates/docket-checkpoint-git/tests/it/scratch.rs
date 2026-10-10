//! Scratch repositories: a temporary folder with its own HOME and git configuration, so the
//! person's real configuration, repositories and index are never read or written.

use docket_checkpoint::WorkRoot;
use docket_checkpoint_git::{GitStore, StdGitRun};
use docket_core::AbsPath;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Scratch {
    dir: tempfile::TempDir,
}

fn git_exists() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

impl Scratch {
    /// A scratch folder, or none when git is missing and the run does not require it.
    pub fn new() -> Option<Scratch> {
        if !git_exists() {
            assert!(
                std::env::var_os("DOCKET_REQUIRE_TOOLS").is_none(),
                "git is required (DOCKET_REQUIRE_TOOLS is set)"
            );
            return None;
        }
        let dir = tempfile::tempdir().expect("scratch folder");
        std::fs::create_dir_all(dir.path().join("home")).expect("home");
        Some(Scratch { dir })
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// The environment git runs in: nothing of the person's configuration.
    pub fn env(&self) -> Vec<(String, String)> {
        let var = |name: &str, value: String| (name.to_owned(), value);
        let home = self.path("home").to_string_lossy().into_owned();
        vec![
            var("HOME", home.clone()),
            var("XDG_CONFIG_HOME", format!("{home}/.config")),
            var("GIT_CONFIG_NOSYSTEM", "1".to_owned()),
            var("GIT_CONFIG_GLOBAL", "/dev/null".to_owned()),
            var(
                "GIT_CEILING_DIRECTORIES",
                self.dir.path().to_string_lossy().into_owned(),
            ),
        ]
    }

    /// Runs git by hand in `cwd`, as a person would; what it printed.
    pub fn git(&self, cwd: &Path, args: &[&str]) -> String {
        let mut command = Command::new("git");
        command.args(args).current_dir(cwd);
        for name in ["GIT_DIR", "GIT_INDEX_FILE", "GIT_WORK_TREE"] {
            command.env_remove(name);
        }
        command.envs(self.env());
        command
            .env("GIT_AUTHOR_NAME", "Tester")
            .env("GIT_AUTHOR_EMAIL", "tester@example.com")
            .env("GIT_COMMITTER_NAME", "Tester")
            .env("GIT_COMMITTER_EMAIL", "tester@example.com");
        let out = command.output().expect("git runs");
        assert!(out.status.success(), "git {args:?} failed");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// A repository with no commits whose `*.log` files are ignored.
    pub fn repo(&self, name: &str) -> PathBuf {
        let repo = self.path(name);
        std::fs::create_dir_all(&repo).expect("repo folder");
        self.git(&repo, &["init", "-q"]);
        std::fs::write(repo.join(".git/info/exclude"), "*.log\n").expect("exclude");
        repo
    }

    /// The store under test, keeping its private indexes in the scratch folder.
    pub fn store(&self) -> GitStore<StdGitRun> {
        let state = self.path("state");
        GitStore::new(StdGitRun, abs(&state)).with_env(self.env())
    }
}

pub fn abs(path: &Path) -> AbsPath {
    AbsPath::parse(&path.to_string_lossy()).expect("an absolute path")
}

pub fn root(path: &Path) -> WorkRoot {
    WorkRoot::new(abs(path))
}
