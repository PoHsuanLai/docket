//! Skills in the app: instructions the app installs for its planner, from text files in the
//! `docket-skills` format. The rule is the desktop's: a skill adds words to the planner's view and
//! reorders what it may call, never a grant, a ruling or a wider policy. The only way skill text
//! enters is a skill directory (`skill.toml` and `SKILL.md`) the app names or a [`Skill`] those
//! files parsed into; nothing a model, a page or a message says can become one.

use crate::agent::InAppAgent;
use crate::sheet::ConfirmSheet;
use action_review::Reviewer;
use docket_client::{ContextSource, IntentProvider};
use docket_core::{PolicyWriter, Reader};
use docket_router::{Clock, GrantStore, MemoryLink};
use docket_skills::{Rejected, Roots, Skill, discover};
use porter_client::Transport as ModelTransport;
use std::path::Path;

impl<P, C, T, R, M: ModelTransport, K, G, Y, W, D> InAppAgent<P, C, T, R, M, K, G, Y, W, D>
where
    P: IntentProvider + 'static,
    C: ContextSource + 'static,
    T: ConfirmSheet + 'static,
    R: Reviewer + 'static,
    K: Clock + Clone + 'static,
    G: GrantStore + 'static,
    Y: MemoryLink + 'static,
    W: PolicyWriter + 'static,
    D: Reader + 'static,
{
    /// Installs `skills` (replacing any installed before): the router knows them (so
    /// `companion.skill.load` can load one) and the planner is offered those whose actions the
    /// app's manifest registers. A skill grants nothing.
    pub fn install_skills(&mut self, skills: Vec<Skill>) {
        self.router.install_skills(skills.clone());
        self.tasks.install_skills(skills);
    }

    /// Installs every skill in the directory `dir` (one subdirectory per skill, named by its id)
    /// and returns the directories that did not load, each with why. A directory that is missing
    /// installs nothing.
    pub fn install_skills_from(&mut self, dir: impl AsRef<Path>) -> Vec<Rejected> {
        let found = discover(&Roots::shipped_only(vec![dir.as_ref().to_owned()]));
        self.install_skills(found.skills);
        found.rejected
    }
}
