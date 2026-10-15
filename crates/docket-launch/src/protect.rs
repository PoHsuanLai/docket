//! Which paths inside the agent's writable workspace are bound read-only again: every `.git`
//! directory, so an agent cannot plant a hook or a config that runs later outside the sandbox
//! (the sandbox half of the checkpoint repository's rule that a hostile `.git` never runs), and
//! any other path the host names (the checkpoint repository, when it sits under the workspace).

use bulkhead::AbsPath;
use std::path::Path;

/// The directory name that is protected wherever it is found.
const GIT: &str = ".git";
/// How far below the workspace the search for nested ones goes.
const DEPTH: usize = 4;
/// How many directory entries the search looks at, so a huge tree does not hold up the start.
const LOOKED_AT: usize = 20_000;

/// The workspace's own `.git` (kept even when it does not exist yet: bulkhead skips a missing
/// path, and a later one is still covered by a nested find on the next start) and every `.git`
/// found below it, never following a link.
pub fn git_dirs(cwd: &AbsPath) -> Vec<AbsPath> {
    let root = Path::new(cwd.as_str());
    let mut found = vec![root.join(GIT)];
    let mut budget = LOOKED_AT;
    nested(root, DEPTH, &mut budget, &mut found);
    found
        .iter()
        .filter_map(|p| p.to_str().and_then(|t| AbsPath::parse(t).ok()))
        .collect()
}

fn nested(dir: &Path, depth: usize, budget: &mut usize, found: &mut Vec<std::path::PathBuf>) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        // `file_type` does not follow a link, so a link out of the workspace is not walked.
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        // A `.git` file (a worktree's link to a repository elsewhere) is protected too.
        if entry.file_name() == GIT {
            found.push(entry.path());
        } else if kind.is_dir() {
            nested(&entry.path(), depth - 1, budget, found);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_git_directory_below_the_workspace_is_found_and_links_are_not_followed() {
        let tmp = tempfile::tempdir().expect("scratch");
        let work = tmp.path().join("work");
        std::fs::create_dir_all(work.join("a/b/.git")).expect("nested");
        std::fs::create_dir_all(work.join("c")).expect("plain");
        std::fs::create_dir_all(tmp.path().join("outside/.git")).expect("outside");
        std::os::unix::fs::symlink(tmp.path().join("outside"), work.join("link")).expect("link");
        let cwd = AbsPath::parse(work.to_str().expect("utf8")).expect("abs");
        let got: Vec<String> = git_dirs(&cwd)
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect();
        let want = [work.join(".git"), work.join("a/b/.git")]
            .map(|p| p.to_str().expect("utf8").to_owned());
        assert_eq!(got, want);
    }
}
