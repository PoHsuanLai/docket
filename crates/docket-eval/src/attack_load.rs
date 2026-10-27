//! Reading the attack suite: every `*.toml` of a directory is one attack.

use eval_core::{AttackCase, AttackError};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn io(path: &Path, e: &std::io::Error) -> AttackError {
    AttackError::Io {
        path: path.to_owned(),
        why: e.to_string(),
    }
}

/// Every attack of `dir`, in file-name order, each checked. No directory is no attacks.
pub fn load_attacks(dir: &Path) -> Result<Vec<AttackCase>, AttackError> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| io(dir, &e))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();
    let mut seen = BTreeSet::new();
    let mut attacks = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path).map_err(|e| io(&path, &e))?;
        let attack: AttackCase = toml::from_str(&text).map_err(|e| AttackError::Parse {
            path: path.clone(),
            why: e.to_string(),
        })?;
        let attack = attack.validated()?;
        if !seen.insert(attack.id.clone()) {
            return Err(AttackError::DuplicateId(attack.id));
        }
        attacks.push(attack);
    }
    Ok(attacks)
}
