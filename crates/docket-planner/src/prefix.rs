//! The part of a planner request that does not change within a task, and where a provider may
//! cache up to. A request is the tools, then the system message, then the user message, and only
//! the user message (the roster, recall, the context, the task's turns and steps, the summary)
//! moves from turn to turn, so the tools and the system message are one prefix and the break comes
//! right after it.
//!
//! The break is a typed value here because the model request type has no slot for it yet. When it
//! has one, the planner sets it from [`PlannerModel::prefix`] and nothing else changes.

use crate::planner::PlannerModel;
use crate::render::system_text_with;
use docket_core::PlannerView;
use porter_client::Transport;
use porter_infer::ToolDecl;
use serde::{Deserialize, Serialize};

/// Where a provider may stop caching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheBreak {
    /// After the tools and the system message: the next message is the first that changes.
    AfterSystem,
}

/// The tools and the system message of a turn, which are byte-identical across the turns of a task
/// while the actions on offer, the pinned profile, the skills, the primer and the digest are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StablePrefix {
    /// The tools, in the view's order.
    pub tools: Vec<ToolDecl>,
    /// The system message: the rules, the agent's role, then what is pinned.
    pub system: String,
    /// Where the cache may end.
    pub break_at: CacheBreak,
}

impl<P: Transport> PlannerModel<P> {
    /// The prefix of the request for `view`.
    pub fn prefix(&self, view: &PlannerView) -> StablePrefix {
        StablePrefix {
            tools: self.tools(view),
            system: system_text_with(view, self.role.as_ref()),
            break_at: CacheBreak::AfterSystem,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_loop::{Sources, assemble};
    use docket_core::{
        AgentConfig, ContextView, Covers, HandleCard, HandleShape, HereView, Reveal, Roster,
        SelectionView, Summary, SummaryText, TextTargetView, TurnSource, TurnVia, UserTurn,
        VisibleView,
    };
    use docket_fake::WordsModel;
    use docket_vocab::{CharCount, Handle, TurnId};
    use porter_core::AppName;
    use porter_ids::Count;
    use prov::{Integrity, Source, UnixSeconds};

    fn sources(window: &str) -> Sources {
        Sources {
            cards: vec![],
            profile: vec![],
            primer: None,
            rollup: None,
            roster: Roster::default(),
            episodes: vec![],
            recalled: vec![],
            context: ContextView {
                app: AppName::parse("org.quire.Shell").expect("app"),
                window: Reveal::Plain(window.into()),
                here: HereView::Nowhere,
                selection: SelectionView::Nothing,
                visible: VisibleView {
                    kind: None,
                    items: vec![],
                    total: Count(0),
                },
                text_target: TextTargetView::None,
            },
            turns: vec![],
            history: vec![],
            handles: vec![],
            inbox: vec![],
            skills: vec![],
            skill_texts: vec![],
            shown: vec![],
            taint: Integrity::Trusted,
            task_policy: None,
            summary: None,
        }
    }

    #[test]
    fn the_prefix_is_byte_identical_across_the_turns_of_a_task() {
        let planner = PlannerModel::new(WordsModel::says("ok"));
        let budget = AgentConfig::default().assembler;
        let first = assemble(&budget, &sources("Inbox"));
        let mut later = sources("A different window");
        later.turns = vec![UserTurn {
            id: TurnId(2),
            text: "and the receipts".into(),
            at: UnixSeconds(9),
            from: TurnSource::Launcher,
            via: TurnVia::Typed,
            mode: None,
        }];
        later.handles = vec![HandleCard {
            handle: Handle(1),
            shape: HandleShape::Text,
            from: Source::User,
            size: CharCount(40),
        }];
        later.summary = Some(Summary {
            covers: Covers {
                turns: Count(0),
                steps: Count(0),
            },
            text: SummaryText::parse("earlier asks", &[]).expect("summary"),
            integrity: Integrity::Trusted,
        });
        let later = assemble(&budget, &later);
        let bytes = |v: &PlannerView| serde_json::to_string(&planner.prefix(v)).expect("json");
        assert_eq!(bytes(&first), bytes(&later));
        assert_eq!(planner.prefix(&first).break_at, CacheBreak::AfterSystem);
        assert_ne!(
            crate::render::user_text(&first),
            crate::render::user_text(&later),
            "what changes is after the break"
        );
    }
}
