//! Finding installed skills. Only directories the caller hands in are read: there is no path from
//! a mail, a web page, a file the person opened or an agent to this module, and nothing here
//! reads the environment (the daemon's `main` does, and passes the results).

use crate::fault::SkillFault;
use crate::skill::{Origin, Skill, parse};
use std::path::{Path, PathBuf};

/// The skill directories, in precedence order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Roots {
    /// The person's own: `$XDG_DATA_HOME/quire/skills`. Overrides the same id elsewhere.
    pub own: Option<PathBuf>,
    /// The shipped ones: `$XDG_DATA_DIRS/*/quire/skills`, earlier first.
    pub shipped: Vec<PathBuf>,
}

impl Roots {
    /// The roots for the given data directories (`$XDG_DATA_HOME` and the entries of
    /// `$XDG_DATA_DIRS`): each gets `quire/skills` appended.
    pub fn in_data_dirs(home: Option<&Path>, dirs: &[PathBuf]) -> Self {
        let skills = |d: &Path| d.join("quire").join("skills");
        Self {
            own: home.map(skills),
            shipped: dirs.iter().map(|d| skills(d)).collect(),
        }
    }

    /// The roots for data directories in precedence order, the person's own first (what intentd
    /// holds as `data_dirs`). Empty: no skills.
    pub fn from_dirs(dirs: &[PathBuf]) -> Self {
        match dirs.split_first() {
            Some((home, rest)) => Self::in_data_dirs(Some(home), rest),
            None => Self::default(),
        }
    }

    /// The roots from an environment lookup, as the XDG base directory spec reads it. Called by
    /// a daemon's `main` with `std::env::var`, never from below.
    pub fn from_env(env: &impl Fn(&str) -> Option<String>) -> Self {
        let set = |key: &str| env(key).filter(|v| !v.is_empty());
        let home = set("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| set("HOME").map(|h| PathBuf::from(h).join(".local").join("share")));
        let dirs = set("XDG_DATA_DIRS")
            .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned())
            .split(':')
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        Self::in_data_dirs(home.as_deref(), &dirs)
    }

    /// Skill directories to read as they are (`docket-eval --check-skills <dirs>`): all shipped.
    pub fn shipped_only(dirs: Vec<PathBuf>) -> Self {
        Self {
            own: None,
            shipped: dirs,
        }
    }
}

/// A skill directory that did not load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    /// The directory.
    pub dir: PathBuf,
    /// Why.
    pub fault: SkillFault,
}

/// What a discovery found: the skills that pass the format, and what did not.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Found {
    /// Valid skills, one per id, by id.
    pub skills: Vec<Skill>,
    /// The directories that failed.
    pub rejected: Vec<Rejected>,
}

fn read(dir: &Path, file: &'static str) -> Result<String, SkillFault> {
    std::fs::read_to_string(dir.join(file)).map_err(|e| SkillFault::Unreadable {
        file,
        why: e.to_string(),
    })
}

/// Loads the skill in `dir` (`skill.toml` and `SKILL.md`; the directory name is its id).
pub fn load_dir(dir: &Path, origin: Origin) -> Result<Skill, SkillFault> {
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    parse(
        name,
        &read(dir, "skill.toml")?,
        &read(dir, "SKILL.md")?,
        origin,
    )
}

fn subdirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// Reads every root: the person's own first, then each shipped root in order. The first valid
/// skill of an id wins; a later one of the same id is shadowed. A directory that fails is
/// reported, and does not shadow anything.
pub fn discover(roots: &Roots) -> Found {
    let own = roots.own.iter().map(|r| (r, Origin::Own));
    let shipped = roots.shipped.iter().map(|r| (r, Origin::Shipped));
    let mut found = Found::default();
    for (root, origin) in own.chain(shipped) {
        for dir in subdirs(root) {
            match load_dir(&dir, origin) {
                Ok(skill) if found.skills.iter().all(|s| s.id != skill.id) => {
                    found.skills.push(skill);
                }
                Ok(_) => {}
                Err(fault) => found.rejected.push(Rejected { dir, fault }),
            }
        }
    }
    found.skills.sort_by(|a, b| a.id.cmp(&b.id));
    found
}
