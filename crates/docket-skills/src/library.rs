//! Checking skills against the registered manifests, and choosing which to show a session.

use crate::fault::Unresolved;
use crate::skill::{Always, Skill};
use docket_core::{ActionRef, AgentReach, SkillCard, SkillId, ValidManifest};
use porter_core::AppName;
use prov::EntityKind;
use std::collections::BTreeSet;

/// The most skills preselected for a turn.
pub const PRESELECT_MAX: usize = 2;
/// The most skills a task may load.
pub const LOAD_MAX: usize = 3;

/// Whether the actions a skill uses can be reached by the companion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// Every action exists and is offered.
    Reachable,
    /// An action is not registered (the app is not installed, or lacks it): invalid.
    Missing(ActionRef),
    /// An action is Hidden from the companion: the skill is not offered, but is not wrong.
    Hidden(ActionRef),
}

/// How `skill`'s `uses` stand against `manifests`.
pub fn reach(skill: &Skill, manifests: &[ValidManifest]) -> Reach {
    for used in &skill.uses {
        let decl = manifests
            .iter()
            .map(ValidManifest::manifest)
            .filter(|m| m.app == used.app)
            .flat_map(|m| &m.actions)
            .find(|a| a.name == used.name);
        match decl {
            None => return Reach::Missing(used.clone()),
            Some(a) if a.reach == AgentReach::Hidden => return Reach::Hidden(used.clone()),
            Some(_) => {}
        }
    }
    Reach::Reachable
}

/// A skill hidden because its `uses` name a missing action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hidden {
    /// Which skill.
    pub id: SkillId,
    /// What is missing.
    pub why: Unresolved,
}

/// The skills of a desktop whose `uses` all exist, and those hidden for a missing action.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Library {
    skills: Vec<Skill>,
    /// What was left out, for the log and `--check-skills`.
    pub hidden: Vec<Hidden>,
}

impl Library {
    /// Keeps the skills whose actions are all registered in `manifests`; reports the others.
    pub fn check(skills: Vec<Skill>, manifests: &[ValidManifest]) -> Self {
        let mut library = Self::default();
        for skill in skills {
            match reach(&skill, manifests) {
                Reach::Missing(action) => library.hidden.push(Hidden {
                    id: skill.id,
                    why: Unresolved::Missing(action),
                }),
                Reach::Reachable | Reach::Hidden(_) => library.skills.push(skill),
            }
        }
        library
    }

    /// Every kept skill, by id.
    pub fn all(&self) -> &[Skill] {
        &self.skills
    }

    /// The skill `id`.
    pub fn get(&self, id: &SkillId) -> Option<&Skill> {
        self.skills.iter().find(|s| &s.id == id)
    }

    /// The skills this session may load: every action is offered (not Hidden for the companion).
    pub fn offered(&self, manifests: &[ValidManifest]) -> Vec<&Skill> {
        self.skills
            .iter()
            .filter(|s| reach(s, manifests) == Reach::Reachable)
            .collect()
    }

    /// The catalogue of `offered`.
    pub fn cards(&self, manifests: &[ValidManifest]) -> Vec<SkillCard> {
        self.offered(manifests).into_iter().map(Skill::card).collect()
    }
}

/// What a turn's context says, for preselection.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Situation {
    /// The app the person is in.
    pub focused: Option<AppName>,
    /// The entity kinds in the context (here, selection, visible).
    pub kinds: BTreeSet<EntityKind>,
}

fn matches(skill: &Skill, at: &Situation) -> bool {
    let app = at
        .focused
        .as_ref()
        .is_some_and(|f| skill.when.focused_app.contains(f));
    let kind = skill.when.kinds.iter().any(|k| at.kinds.contains(k));
    skill.when.always == Always::Yes || app || kind
}

/// The skills to expand at the start of a turn: those whose `when` matches (`always`, the focused
/// app, or a kind in the context), `always` ones first, then by id, at most two.
pub fn preselect<'a>(offered: &[&'a Skill], at: &Situation) -> Vec<&'a Skill> {
    let mut hits: Vec<&Skill> = offered.iter().copied().filter(|s| matches(s, at)).collect();
    hits.sort_by_key(|s| (s.when.always != Always::Yes, s.id.clone()));
    hits.truncate(PRESELECT_MAX);
    hits
}

/// Why a load was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LoadRefusal {
    /// Three skills are already loaded in this task.
    #[error("this task has already loaded {LOAD_MAX} skills")]
    OverCap,
}

/// The skills one task has loaded, in order. Loading one again costs nothing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Loaded(Vec<SkillId>);

impl Loaded {
    /// Admits `id`, or refuses beyond the cap.
    pub fn admit(&mut self, id: &SkillId) -> Result<(), LoadRefusal> {
        if self.0.contains(id) {
            return Ok(());
        }
        if self.0.len() >= LOAD_MAX {
            return Err(LoadRefusal::OverCap);
        }
        self.0.push(id.clone());
        Ok(())
    }

    /// The loaded ids, oldest first.
    pub fn ids(&self) -> &[SkillId] {
        &self.0
    }
}

/// `items` with those whose action a loaded skill uses first, each group in its own order.
/// Nothing is added or removed: the list is only reordered.
pub fn uses_first<T>(
    items: Vec<T>,
    action: impl Fn(&T) -> &ActionRef,
    loaded: &[&Skill],
) -> Vec<T> {
    let used: BTreeSet<&ActionRef> = loaded.iter().flat_map(|s| &s.uses).collect();
    let (first, rest): (Vec<T>, Vec<T>) = items.into_iter().partition(|i| used.contains(action(i)));
    first.into_iter().chain(rest).collect()
}
