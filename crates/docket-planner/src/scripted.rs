//! A planner that plays what it was given, for tests of everything that drives a planner (the task
//! loop, the evaluation harness) and have no use for a transport. It keeps the catalogue, the
//! choices and the voice it is set to, hands out its scripted turns in order, and remembers the
//! views it was shown. Out of turns, a planner turn is [`PlanFault::Unavailable`]; it has no
//! chat model, so a chat or a summary it was not given is unavailable too.

use crate::catalogue::Catalogue;
use crate::choices::ParamChoices;
use crate::planner::{PlanFault, PlannerReply};
use crate::role::RoleText;
use crate::seam::Planner;
use crate::summarise::{SummariseFault, Summarised};
use crate::voice::Voice;
use agent_loop::SummaryInput;
use docket_core::PlannerView;
use porter_infer::{ChatReply, ChatRequest, OpenOptions};
use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// The turns a [`ScriptedPlanner`] has left to play, and the views it was shown.
#[derive(Debug, Default)]
struct Script {
    turns: VecDeque<Result<PlannerReply, PlanFault>>,
    summaries: VecDeque<Result<Summarised, SummariseFault>>,
    seen: Vec<PlannerView>,
}

/// A [`Planner`] that answers from a script.
#[derive(Debug)]
pub struct ScriptedPlanner {
    catalogue: Catalogue,
    choices: Vec<ParamChoices>,
    voice: Voice,
    script: Mutex<Script>,
}

impl Default for ScriptedPlanner {
    fn default() -> Self {
        Self {
            catalogue: Catalogue::default(),
            choices: Vec::new(),
            voice: Voice {
                role: None,
                options: OpenOptions::default(),
            },
            script: Mutex::new(Script::default()),
        }
    }
}

impl ScriptedPlanner {
    /// A planner that plays `turns` in order, one per `converse`.
    pub fn playing(turns: impl IntoIterator<Item = Result<PlannerReply, PlanFault>>) -> Self {
        let planner = Self::default();
        planner.script().turns.extend(turns);
        planner
    }

    /// The same planner with the summaries it hands out, in order, one per `summarise`.
    pub fn with_summaries(
        self,
        summaries: impl IntoIterator<Item = Result<Summarised, SummariseFault>>,
    ) -> Self {
        self.script().summaries.extend(summaries);
        self
    }

    /// The views it was shown, oldest first.
    pub fn seen(&self) -> Vec<PlannerView> {
        self.script().seen.clone()
    }

    /// The choices the planner was last given.
    pub fn choices(&self) -> &[ParamChoices] {
        &self.choices
    }

    fn script(&self) -> MutexGuard<'_, Script> {
        self.script.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Planner for ScriptedPlanner {
    fn catalogue(&self) -> &Catalogue {
        &self.catalogue
    }

    fn set_catalogue(&mut self, catalogue: Catalogue) {
        self.catalogue = catalogue;
    }

    fn set_choices(&mut self, choices: Vec<ParamChoices>) {
        self.choices = choices;
    }

    fn voice(&self) -> Voice {
        self.voice.clone()
    }

    fn set_voice(&mut self, voice: Voice) {
        self.voice = voice;
    }

    fn set_role(&mut self, role: RoleText) {
        self.voice.role = Some(role);
    }

    async fn converse(&self, view: &PlannerView) -> Result<PlannerReply, PlanFault> {
        let mut script = self.script();
        script.seen.push(view.clone());
        script
            .turns
            .pop_front()
            .unwrap_or(Err(PlanFault::Unavailable))
    }

    async fn chat(&self, _request: ChatRequest) -> Result<ChatReply, PlanFault> {
        Err(PlanFault::Unavailable)
    }

    async fn summarise(&self, _input: &SummaryInput) -> Result<Summarised, SummariseFault> {
        self.script()
            .summaries
            .pop_front()
            .unwrap_or(Err(SummariseFault::Model(PlanFault::Unavailable)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use porter_core::consent::Usage;
    use porter_core::{DataClass, Tier};

    #[test]
    fn a_role_set_is_the_voice_it_speaks_in() {
        let mut planner = ScriptedPlanner::default();
        assert_eq!(planner.voice().role, None);
        let role = RoleText::parse("You tidy the inbox.").expect("role");
        planner.set_role(role.clone());
        assert_eq!(planner.voice().role, Some(role));
    }

    #[tokio::test]
    async fn a_planner_with_nothing_scripted_is_unavailable() {
        let planner = ScriptedPlanner::default();
        let request = ChatRequest::new(
            Vec::new(),
            Tier::Balanced,
            DataClass::Prompt,
            Usage::Interactive,
        );
        assert_eq!(
            planner.chat(request).await.err(),
            Some(PlanFault::Unavailable)
        );
        assert_eq!(planner.seen(), Vec::new());
    }
}
