//! Compaction in the driver: before a planner turn, a task that has outgrown its budget has its
//! oldest whole steps summarised once, and the summary stands in for them from then on. The choice
//! of what to cover and what the summariser may see are `agent-loop`'s; the model turn is the
//! planner's (`PlannerModel::summarise`). The task keeps every step it ever had: the summary is
//! only what the assembler shows in their place.
//!
//! A summary that cannot be made (the model is away, was cancelled, or wrote something the checks
//! refuse) leaves the task as it was, and the assembler falls back to dropping the oldest masked
//! steps.

use crate::deadline::bounded;
use crate::runtime::Companion;
use crate::seams::Surface;
use agent_loop::{Sources, compaction_due, summarised, summary_input};
use docket_client::Transport as IntentsTransport;
use docket_core::Clock;
use docket_core::Summary;
use porter_client::Transport as InferTransport;
use prov::TaskId;

impl<P: InferTransport, I: IntentsTransport, K: Clock, S: Surface> Companion<P, I, K, S> {
    /// The summary that now stands for the start of `task`, when it was over its budget and one
    /// was made; `None` when nothing was due or the attempt failed.
    pub(crate) async fn compact(&mut self, task: &TaskId, sources: &Sources) -> Option<Summary> {
        let covers = compaction_due(&self.config.assembler, sources)?;
        let input = summary_input(sources, covers);
        let asked = bounded(
            &*self.shared,
            &self.clock,
            task,
            self.waits.model,
            self.planner.summarise(&input),
        )
        .await;
        let made = match asked {
            Ok(Ok(made)) => made,
            Ok(Err(fault)) => {
                tracing::warn!(%task, %fault, "the task's summary could not be made");
                return None;
            }
            Err(_) => return None,
        };
        let summary = summarised(&input, made.text);
        if let Some(rt) = self.runtimes.get_mut(task) {
            rt.summary = Some(summary.clone());
        }
        Some(summary)
    }
}

#[cfg(test)]
mod tests {
    use crate::{Companion, Quiet};
    use agent_loop::compaction_due;
    use docket_client::{InProcess, Intents};
    use docket_core::{
        ActionRef, AgentConfig, CallId, CallerId, CallerRole, LabelText, SessionOpen, StepEnd,
        StepLine, StepShown,
    };
    use docket_fake::{FixedClock, WordsModel, fake_router};
    use docket_planner::PlannerModel;
    use porter_core::{AppId, AppName, Isolation, Tokens};
    use prov::{ActionName, AgentRef, Effect, SpaceId, UnixSeconds};
    use std::{collections::BTreeSet, sync::Arc};

    fn step(n: u64) -> StepLine {
        StepLine {
            call: CallId(n),
            action: ActionRef {
                app: AppName::parse("org.quire.Shell").expect("app"),
                name: ActionName::parse("demo.item.archive").expect("action"),
            },
            effect: Effect::UndoableWrite,
            end: StepEnd::Done {
                said: Some(LabelText::parse("Archived 12 items").expect("said")),
                value: None,
                undo: None,
            },
            shown: StepShown::Full,
            with: Vec::new(),
        }
    }

    #[tokio::test]
    async fn a_task_over_its_budget_is_summarised_once_and_keeps_every_step() {
        let router = Arc::new(fake_router(AgentConfig::default()).expect("router"));
        let shell = AppName::parse("org.quire.Shell").expect("app");
        let caller = CallerId {
            app: AppId {
                name: shell.clone(),
                isolation: Isolation::Unsandboxed,
            },
            roles: BTreeSet::from([CallerRole::Companion]),
        };
        let mut companion: Companion<_, _, FixedClock, Quiet> = Companion::new(
            Intents::over(InProcess::new(router, caller)),
            PlannerModel::new(WordsModel::says("Archived the earlier items in batches.")),
            AgentConfig::default(),
            FixedClock::at(UnixSeconds(1_000)),
            shell,
        );
        let opened = companion
            .open(SessionOpen {
                space: SpaceId::parse("work").expect("space"),
                agent: AgentRef::Companion,
                parent: None,
                cwd: None,
                started_from: None,
                external: None,
                helper: None,
                acts_for: None,
            })
            .await
            .expect("open");
        let task = opened.task.clone();
        companion.runtimes.get_mut(&task).expect("runtime").history = (1..=40).map(step).collect();
        companion.config.assembler.task = Tokens(700);

        let sources = companion.sources(&task).await;
        let summary = companion
            .compact(&task, &sources)
            .await
            .expect("over its budget, so summarised");
        assert!(summary.covers.steps.0 >= 1 && summary.covers.steps.0 <= 37);
        assert_eq!(
            companion.runtimes[&task].history.len(),
            40,
            "no step is lost"
        );
        assert_eq!(companion.runtimes[&task].summary, Some(summary.clone()));

        let again = companion.sources(&task).await;
        assert_eq!(again.summary, Some(summary));
        assert_eq!(compaction_due(&companion.config.assembler, &again), None);
        assert_eq!(
            companion.compact(&task, &again).await,
            None,
            "once is enough"
        );
    }
}
