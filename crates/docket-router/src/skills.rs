//! `companion.skill.load`: hands the planner an installed skill's body as trusted text. A skill
//! only teaches, so the call is a Read like any other (gated, audited), it adds no grant and
//! removes no check, and a task may load at most three.
//!
//! The skills are the valid files intentd found in the data directories at start
//! (`install_skills`); this module never reads a file. A skill is loadable only when every action
//! it uses is registered and offered to the companion, so a skill cannot point the planner at an
//! action that does not exist or is hidden.

use crate::router::Router;
use crate::seams::Seams;
use docket_core::{
    AppRefusal, FailText, Follow, Invocation, LabelText, Outcome, ParamName, Preview, SkillId,
    Undoable, ValidManifest, Value,
};
use docket_skills::{Library, Skill};
use prov::{Actor, Labelled};

/// The action's name.
pub const SKILL_LOAD: &str = "companion.skill.load";

fn failed(why: &str) -> AppRefusal {
    AppRefusal::Failed(FailText(why.to_owned()))
}

impl<S: Seams> Router<S> {
    /// Sets the installed skills. Called once by the daemon that read them, after the manifests
    /// are registered.
    pub fn install_skills(&self, skills: Vec<Skill>) {
        self.locked().skills = skills;
    }

    /// The skills that are installed but hidden because an action they use is not registered,
    /// as lines for the daemon's log.
    pub fn hidden_skills(&self) -> Vec<String> {
        let st = self.locked();
        let manifests: Vec<ValidManifest> = st.registry.all().cloned().collect();
        Library::check(st.skills.clone(), &manifests)
            .hidden
            .iter()
            .map(|h| format!("skill {} hidden: {}", h.id, h.why))
            .collect()
    }

    pub(crate) fn skill_load(&self, inv: &Invocation) -> Result<Outcome, AppRefusal> {
        let Actor::Companion { session, .. } = &inv.actor else {
            return Err(AppRefusal::Unsupported);
        };
        let param = ParamName::parse("id").map_err(|_| AppRefusal::Unsupported)?;
        let id = match inv.args.get(&param).map(|a| &a.value) {
            Some(Value::Text(t)) => {
                SkillId::parse(t).map_err(|_| failed("that is not a skill id"))?
            }
            _ => {
                return Err(AppRefusal::NeedsParam {
                    param,
                    options: Vec::new(),
                });
            }
        };
        let mut st = self.locked();
        let manifests: Vec<ValidManifest> = st.registry.all().cloned().collect();
        let library = Library::check(st.skills.clone(), &manifests);
        let skill = library
            .offered(&manifests)
            .into_iter()
            .find(|s| s.id == id)
            .cloned()
            .ok_or_else(|| failed("there is no such skill"))?;
        let record = st
            .sessions
            .get_mut(session)
            .ok_or_else(|| failed("the session is gone"))?;
        record
            .skill_loads
            .admit(&id)
            .map_err(|e| failed(&e.to_string()))?;
        let text = skill.text();
        Ok(Outcome {
            value: Some(Labelled {
                value: Value::Text(text.body),
                label: text.label,
            }),
            said: LabelText::parse(&format!("Loaded the {} skill", skill.id)).ok(),
            show: Preview::None,
            undo: Undoable::No,
            follow: Follow::Nothing,
        })
    }
}
