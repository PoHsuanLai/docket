//! The person's standing consent for actions, in a file under the user's data directory.

use crate::defaults::{default_grants_file, read_defaults};
use crate::policies::{PolicyDirs, load_policies};
use docket_core::{
    ActionGrant, Ended, GrantFileError, KnownSpaces, Limit, LimitCaller, LimitKey, LimitLevel,
    LimitMaker, LimitScope, Reconciled, Revocation, RuleFile, SpaceAccess, StandingGrant,
    StandingGrantId, decode_grants, decode_limits, decode_standing, encode_limits, encode_standing,
    held_with, held_without, limits_with, limits_without, read_grant_file, read_grant_list,
    read_optional, reconcile, with_rule, without_rule, without_space, write_atomic,
    write_grant_list,
};
use docket_router::GrantStore;
use policy_point::Presets;
use prov::{Effect, SpaceId, SpaceScope, UnixSeconds};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One writer at a time: a grant recorded while another is being written is added to the file
/// the first left, not to the one it read before.
static WRITING: Mutex<()> = Mutex::new(());

/// Grants kept in `$XDG_DATA_HOME/quire/intents/grants.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileGrants {
    path: PathBuf,
    /// The shipped defaults files, read-only, layered under what the person's file holds.
    defaults: Vec<PathBuf>,
    /// Where the rules files are: the person's directory and the assistant's inside it. None
    /// writes no rules.
    rules: Option<PolicyDirs>,
}

impl FileGrants {
    /// Grants stored at `path`.
    pub fn at(path: PathBuf) -> Self {
        Self {
            path,
            defaults: Vec::new(),
            rules: None,
        }
    }

    /// The same store writing "never" and "ask first" rules into the rules files of `dirs`
    /// (`never.cedar` in the person's, `assistant.cedar` in the assistant's), and reading the
    /// policy back from them.
    pub fn with_rules(self, dirs: PolicyDirs) -> Self {
        Self {
            rules: Some(dirs),
            ..self
        }
    }

    /// The same store with the shipped defaults of each data directory under it
    /// (`quire/intents/default-grants.json`): the person's own entries, a denial included,
    /// always win over a default.
    pub fn with_defaults(self, data_dirs: &[PathBuf]) -> Self {
        Self {
            defaults: data_dirs.iter().map(|d| default_grants_file(d)).collect(),
            ..self
        }
    }

    /// What the file holds: nothing when it is missing. A file that cannot be read or is not a
    /// list of grants is a fault, never an empty list. An entry for a Space porter does not read
    /// is not in the answer (it is dropped, and counted in `ended`).
    fn read(&self) -> Result<Reconciled, GrantFileError> {
        read_grant_file(&self.path, Reconciled::default(), decode_grants)
    }

    /// The grants that stay: those no other app's Space is under.
    fn kept(&self) -> Result<Vec<ActionGrant>, GrantFileError> {
        self.read().map(|read| reconcile(read.kept, None).kept)
    }

    /// Rewrites the file when `settle` ends any grant, and says what ended (what the file could
    /// not read included). A file that cannot be read is left alone.
    fn settle(
        &self,
        settle: impl FnOnce(Vec<ActionGrant>) -> Reconciled,
    ) -> Result<Vec<Ended>, GrantFileError> {
        let _one_at_a_time = WRITING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let read = self.read()?;
        let settled = settle(read.kept);
        let ended: Vec<Ended> = read.ended.into_iter().chain(settled.ended).collect();
        if !ended.is_empty() {
            self.write(&settled.kept)?;
        }
        Ok(ended)
    }

    /// Drops every grant over a desktop-wide Space `known` does not hold (and any over another
    /// app's own Space), and those the file holds for a Space porter does not read. Run at
    /// start: a Space removed while the daemon was away ends its grants here.
    pub fn reconcile_with(&self, known: &KnownSpaces) -> Result<Vec<Ended>, GrantFileError> {
        self.settle(|grants| reconcile(grants, Some(known)))
    }

    /// Drops every grant scoped to exactly `gone`: the Space was removed.
    pub fn end_space(&self, gone: &SpaceId) -> Result<Vec<Ended>, GrantFileError> {
        self.settle(|grants| without_space(grants, gone))
    }

    /// Removes the rule `id` from the rules file that holds it. A file that cannot be read is
    /// left alone.
    fn revoke_rule(&self, id: &StandingGrantId) -> Revocation {
        let Some(dirs) = &self.rules else {
            return Revocation::NotHeld;
        };
        [RuleFile::Never, RuleFile::Assistant]
            .into_iter()
            .map(|file| rule_path(dirs, file))
            .find_map(|path| {
                let text = read_optional(&path).ok().flatten()?;
                let (rest, done) = without_rule(&text, id);
                (done == Revocation::Revoked).then(|| {
                    logged(put_rules(&path, &rest));
                    done
                })
            })
            .unwrap_or(Revocation::NotHeld)
    }

    fn standing_path(&self) -> PathBuf {
        self.path.with_file_name("standing.json")
    }

    fn read_standing(&self) -> Result<Vec<StandingGrant>, GrantFileError> {
        read_grant_list(&self.standing_path(), decode_standing)
    }

    fn write_standing(&self, grants: &[StandingGrant]) -> Result<(), GrantFileError> {
        write_grant_list(&self.standing_path(), encode_standing(grants))
    }

    fn limits_path(&self) -> PathBuf {
        self.path.with_file_name("limits.json")
    }

    fn read_limits(&self) -> Result<Vec<Limit>, GrantFileError> {
        read_grant_list(&self.limits_path(), decode_limits)
    }

    fn write_limits(&self, limits: &[Limit]) -> Result<(), GrantFileError> {
        write_grant_list(&self.limits_path(), encode_limits(limits))
    }

    fn write(&self, grants: &[ActionGrant]) -> Result<(), GrantFileError> {
        write_grant_list(&self.path, serde_json::to_string_pretty(grants))
    }
}

/// The path of a rules file under `dirs`.
fn rule_path(dirs: &PolicyDirs, file: RuleFile) -> PathBuf {
    match file {
        RuleFile::Never => dirs.person.join(file.name()),
        RuleFile::Assistant => dirs.assistant().join(file.name()),
    }
}

/// Replaces the rules file `path` with `text`, whole or not at all.
fn put_rules(path: &Path, text: &str) -> Result<(), GrantFileError> {
    write_atomic(path, text.as_bytes()).map_err(|why| GrantFileError::Write {
        path: path.to_owned(),
        kind: why.kind(),
    })
}

/// Rewrites the rules file `path` with `edit` of its text. A file that cannot be read is not
/// written over.
fn edit_rules(path: &Path, edit: impl FnOnce(&str) -> String) -> Result<(), GrantFileError> {
    let text = read_optional(path)
        .map_err(|why| GrantFileError::Read {
            path: path.to_owned(),
            kind: why.kind(),
        })?
        .unwrap_or_default();
    put_rules(path, &edit(&text))
}

/// The limit a damaged limits file stands for: every agent call asks.
fn ask_about_everything() -> Limit {
    let key = LimitKey {
        caller: LimitCaller::AnyAgent,
        scope: LimitScope::Everything,
        space: SpaceScope::Any,
        from: Effect::Read,
        level: LimitLevel::AlwaysAsk,
    };
    Limit::new(key, LimitMaker::Person, None, UnixSeconds(0))
}

/// What a store does with a fault: the person is asked again (no grants held), and the line says
/// why. The damaged file is left alone for the person to look at.
fn logged<T>(done: Result<T, GrantFileError>) -> Option<T> {
    done.map_err(|why| eprintln!("intentd: {why}")).ok()
}

impl GrantStore for FileGrants {
    fn grants(&self) -> Vec<ActionGrant> {
        let mut all: Vec<ActionGrant> = self
            .defaults
            .iter()
            .flat_map(|f| read_defaults(f))
            .collect();
        all.extend(logged(self.kept()).unwrap_or_default());
        all
    }

    fn record(&self, grant: ActionGrant) {
        // An app holds no grant over another app's own Space.
        if let SpaceAccess::Refused(_) = grant.key.space_access() {
            return;
        }
        let _one_at_a_time = WRITING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // A file that cannot be read is not written over: the grant is not kept, and the
        // person is asked again.
        let Some(mut all) = logged(self.kept()) else {
            return;
        };
        all.retain(|held| held.key != grant.key);
        all.push(grant);
        logged(self.write(&all));
    }

    // Standing grants ("allow always", scoped) in `standing.json` beside the grants file, read
    // afresh on every call: a revocation takes effect on the next call, and a damaged file holds
    // none (which only means the person is asked again) and is never written over.
    fn standing(&self) -> Vec<StandingGrant> {
        logged(self.read_standing()).unwrap_or_default()
    }

    fn add_standing(&self, grant: StandingGrant) {
        let _one_at_a_time = WRITING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(held) = logged(self.read_standing()) {
            logged(self.write_standing(&held_with(held, grant)));
        }
    }

    // Limits ("always ask", "never") in `limits.json` beside them, read afresh on every call and
    // never written over when damaged. Unlike a grant, a limit that is lost loosens things, so a
    // file that cannot be read stands for one limit that asks about everything an agent does,
    // until the person looks at the file.
    fn limits(&self) -> Vec<Limit> {
        logged(self.read_limits()).unwrap_or_else(|| vec![ask_about_everything()])
    }

    fn add_limit(&self, limit: Limit) {
        let _one_at_a_time = WRITING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(held) = logged(self.read_limits()) {
            logged(self.write_limits(&limits_with(held, limit)));
        }
    }

    // One revoke path for both: the id says which file it is in, and the administrator's locked
    // entries are in neither.
    fn revoke_standing(&self, id: &StandingGrantId) -> Revocation {
        let _one_at_a_time = WRITING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(held) = logged(self.read_standing()) {
            let (rest, done) = held_without(held, id);
            if done == Revocation::Revoked {
                logged(self.write_standing(&rest));
                return done;
            }
        }
        if let Some(held) = logged(self.read_limits()) {
            let (rest, done) = limits_without(held, id);
            if done == Revocation::Revoked {
                logged(self.write_limits(&rest));
                return done;
            }
        }
        self.revoke_rule(id)
    }

    // A limit is a forbid rule in a rules file; the file is read back so a hand-edited file keeps
    // what the person wrote beside it.
    fn add_rule(&self, limit: &Limit) {
        let (Some(dirs), Some(file)) = (&self.rules, RuleFile::of(limit.by)) else {
            return;
        };
        let _one_at_a_time = WRITING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        logged(edit_rules(&rule_path(dirs, file), |text| {
            with_rule(text, limit)
        }));
    }

    fn rebuilt(&self) -> Option<Presets> {
        match load_policies(self.rules.as_ref()?) {
            Ok(loaded) => Some(loaded.presets),
            Err(why) => {
                eprintln!("intentd: policies not reloaded, the set in force stays: {why}");
                None
            }
        }
    }
}
