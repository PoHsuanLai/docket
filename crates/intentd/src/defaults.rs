//! Shipped consent: a read-only `quire/intents/default-grants.json` under each data directory,
//! layered under the person's own grants file.
//!
//! A default is a grant dated the epoch, so the person's own entry for the same key (newer by
//! construction) always wins, and a denial the person records wins over a default (see
//! [`revoking`]). Only the narrowest kind of default is accepted: an `Always` allowance over the
//! `AppOwn` class, which is the data an app made itself (for the shell: window and menu state).
//! Anything else in the file is skipped with a line on standard error, so a defaults file can
//! never lend the companion mail, files or the screen.

use docket_core::ActionGrant;
use porter_core::DataClass;
use porter_core::GrantId;
use porter_core::consent::{Decision, Grant, GrantScope};
use prov::UnixSeconds;
use std::path::{Path, PathBuf};

/// The file name of the shipped defaults, in `quire/intents` of a data directory.
pub const DEFAULT_GRANTS_FILE: &str = "default-grants.json";

/// Where a data directory keeps the shipped defaults.
pub fn default_grants_file(data: &Path) -> PathBuf {
    crate::manifests::intents_dir(data).join(DEFAULT_GRANTS_FILE)
}

/// Whether a default may say this: an allowance, lasting, over `AppOwn` only.
fn acceptable(grant: &ActionGrant) -> bool {
    grant.decision == Decision::Allow
        && grant.scope == GrantScope::Always
        && grant.key.class == DataClass::AppOwn
}

/// The defaults `text` holds, dated the epoch so that every entry of the person's outranks
/// them, and how many entries were too broad to accept.
fn accepted(text: &str) -> Result<(Vec<ActionGrant>, usize), serde_json::Error> {
    let all: Vec<ActionGrant> = serde_json::from_str(text)?;
    let total = all.len();
    let kept: Vec<ActionGrant> = all
        .into_iter()
        .filter(acceptable)
        .map(|grant| Grant {
            at: UnixSeconds(0),
            ..grant
        })
        .collect();
    let refused = total - kept.len();
    Ok((kept, refused))
}

/// What the defaults file at `path` holds: nothing when it is missing or damaged (the person is
/// then asked, which is the safe side).
pub(crate) fn read_defaults(path: &Path) -> Vec<ActionGrant> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(why) => {
            eprintln!("intentd: cannot read {}: {why}", path.display());
            return Vec::new();
        }
    };
    match accepted(&text) {
        Ok((kept, 0)) => kept,
        Ok((kept, refused)) => {
            eprintln!(
                "intentd: {}: {refused} default(s) skipped, only Always allowances over app_own are accepted",
                path.display()
            );
            kept
        }
        Err(why) => {
            eprintln!("intentd: {} is not a list of grants: {why}", path.display());
            Vec::new()
        }
    }
}

/// The person's revocation of `default`: a denial for exactly its key, dated `at`, which
/// outranks the default. Recorded in the person's grants file; the shipped file is never written.
pub fn revoking(default: &ActionGrant, id: GrantId, at: UnixSeconds) -> ActionGrant {
    Grant {
        id,
        key: default.key.clone(),
        decision: Decision::Deny,
        scope: GrantScope::Always,
        at,
    }
}
