//! Finding coding agents that are already on this computer, by name, on the directories of
//! `PATH`. Nothing is run: a name is looked for as a file and that is all. Finding one does not
//! list it: only the person's `agents.toml` lists a program that may start, so a found program
//! that no entry runs is offered as "not set up yet" and nothing more.

use super::model::Agents;
use std::path::{Path, PathBuf};

/// A coding agent program known by the name it is installed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Known {
    /// The file name.
    pub program: &'static str,
    /// What the person calls it.
    pub label: &'static str,
}

/// The programs looked for.
pub const KNOWN: [Known; 4] = [
    Known {
        program: "claude-agent-acp",
        label: "Claude Code",
    },
    Known {
        program: "claude",
        label: "Claude Code",
    },
    Known {
        program: "gemini",
        label: "Gemini",
    },
    Known {
        program: "codex",
        label: "Codex",
    },
];

/// A known program found on a directory of `PATH`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Which program.
    pub known: Known,
    /// Where.
    pub at: PathBuf,
}

/// The directories `path` (the value of `PATH`) names.
pub fn dirs_of(path: &str) -> Vec<PathBuf> {
    std::env::split_paths(path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .collect()
}

/// Every known program that is a file in one of `dirs`, the first directory with it winning;
/// `is_file` is the look at the disk.
pub fn find_known(dirs: &[PathBuf], is_file: impl Fn(&Path) -> bool) -> Vec<Found> {
    KNOWN
        .into_iter()
        .filter_map(|known| {
            dirs.iter()
                .map(|dir| dir.join(known.program))
                .find(|at| is_file(at))
                .map(|at| Found { known, at })
        })
        .collect()
}

/// The found programs that no entry of `agents` runs: the same file, or one of the same name.
pub fn unlisted<'a>(agents: &Agents, found: &'a [Found]) -> Vec<&'a Found> {
    let runs = |found: &Found| {
        agents
            .rows
            .iter()
            .filter_map(|row| row.command.as_deref())
            .any(|command| {
                let command = Path::new(command);
                command == found.at || command.file_name() == found.at.file_name()
            })
    };
    found.iter().filter(|one| !runs(one)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use docket_agents::AgentsDir;

    fn dirs() -> Vec<PathBuf> {
        dirs_of("/first/bin::/second/bin")
    }

    #[test]
    fn path_is_split_into_its_directories_and_an_empty_one_is_dropped() {
        assert_eq!(
            dirs(),
            vec![PathBuf::from("/first/bin"), PathBuf::from("/second/bin")]
        );
    }

    #[test]
    fn a_known_program_is_found_in_the_first_directory_that_has_it() {
        let present = [
            "/second/bin/claude-agent-acp",
            "/first/bin/claude-agent-acp",
            "/second/bin/codex",
        ];
        let found = find_known(&dirs(), |at| present.iter().any(|p| Path::new(p) == at));
        let seen: Vec<(&str, &Path)> = found
            .iter()
            .map(|one| (one.known.program, one.at.as_path()))
            .collect();
        assert_eq!(
            seen,
            [
                ("claude-agent-acp", Path::new("/first/bin/claude-agent-acp")),
                ("codex", Path::new("/second/bin/codex")),
            ]
        );
    }

    #[test]
    fn a_found_program_that_an_entry_runs_is_listed_and_one_that_none_runs_is_not() {
        let file = "[[agent]]\nprogram = \"claude-code\"\ncommand = \"/opt/x/claude-agent-acp\"\n";
        let tmp = std::env::temp_dir();
        let agents = crate::agents::read_agents(file, &AgentsDir::at(&tmp.join("none")));
        let found = vec![
            Found {
                known: KNOWN[0],
                at: PathBuf::from("/first/bin/claude-agent-acp"),
            },
            Found {
                known: KNOWN[3],
                at: PathBuf::from("/first/bin/codex"),
            },
        ];
        let names: Vec<&str> = unlisted(&agents, &found)
            .into_iter()
            .map(|one| one.known.program)
            .collect();
        assert_eq!(names, ["codex"]);
    }
}
