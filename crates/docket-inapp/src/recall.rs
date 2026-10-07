//! What the planner is shown of the world around the turn: the task's own words and history, and
//! what memory holds for the Space (the primer, the person's profile, recent episodes, and the
//! hits that bear on what was said). Every memory section goes through the router's
//! `Session.Recall`, which labels it: untrusted text arrives as a handle, never as itself. A
//! source that does not answer leaves its section empty (the planner works with less, never with
//! a guess), which is all the stub memory ever does.
//!
//! The limits are companiond's (`sources.rs`): the same question asked in-process.

use crate::agent::InAppAgent;
use crate::sheet::ConfirmSheet;
use crate::turn::OpenTask;
use action_review::Reviewer;
use agent_loop::Sources;
use almanac_core::{BodyMode, InjectQuery, RecallOver, RecentQuery, TrustFilter, UserText};
use docket_client::{ContextSource, IntentProvider};
use docket_core::{
    ContextView, EpisodeLine, HereView, PolicyWriter, PrimerText, ProfileLine, Reader, RecallAsk,
    RecallView, RecalledLine, Reveal, SelectionView, TextTargetView, VisibleView,
};
use docket_router::{Clock, GrantStore, MemoryLink};
use porter_client::Transport as ModelTransport;
use porter_core::{AppName, Count};
use prov::UnixSeconds;

/// How many hits the automatic recall asks for before the token budget cuts them.
const RECALL_K: Count = Count(8);
/// How many recent episodes the planner is shown.
const RECENT_LIMIT: Count = Count(20);
/// How far back the recent episodes reach, in seconds.
const RECENT_WINDOW: i64 = 24 * 3600;

fn nowhere(app: AppName) -> ContextView {
    ContextView {
        app,
        window: Reveal::Plain(String::new()),
        here: HereView::Nowhere,
        selection: SelectionView::Nothing,
        visible: VisibleView {
            kind: None,
            items: Vec::new(),
            total: Count(0),
        },
        text_target: TextTargetView::None,
    }
}

fn words_of(task: &OpenTask) -> String {
    task.turns
        .iter()
        .map(|t| t.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

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
    /// The sections for one planner turn of `task`.
    pub(crate) async fn sources(&self, task: &OpenTask) -> Sources {
        let task_policy = self
            .intents
            .session_task_policy(task.session.clone())
            .await
            .unwrap_or_default();
        let (primer, profile) = self.memory_sections(task).await;
        Sources {
            cards: self.planner.catalogue().cards(),
            profile,
            primer,
            rollup: None,
            roster: docket_core::Roster::default(),
            episodes: self.episodes_for(task).await,
            recalled: self.recall_for(task).await,
            context: nowhere(self.app.clone()),
            turns: task.turns.clone(),
            history: task.history.clone(),
            handles: task.handles.clone(),
            inbox: Vec::new(),
            taint: task.taint(),
            task_policy,
            skills: Vec::new(),
            skill_texts: Vec::new(),
        }
    }

    /// The Space's primer and the person's own profile.
    async fn memory_sections(&self, task: &OpenTask) -> (Option<PrimerText>, Vec<ProfileLine>) {
        let primer = match self
            .intents
            .session_recall(task.session.clone(), RecallAsk::Primer)
            .await
        {
            Ok(RecallView::Primer(text)) if !text.0.is_empty() => Some(text),
            _ => None,
        };
        let profile = match self
            .intents
            .session_recall(task.session.clone(), RecallAsk::Profile)
            .await
        {
            Ok(RecallView::Profile(lines)) => lines,
            _ => Vec::new(),
        };
        (primer, profile)
    }

    /// What recently ended in the Space: the skeletons as trusted lines, a narrative only by
    /// handle. The tasks this agent ended are there once their audit was written
    /// (`AuditTo::Memory`).
    async fn episodes_for(&self, task: &OpenTask) -> Vec<EpisodeLine> {
        let now = self.clock.now();
        let query = RecentQuery {
            since: UnixSeconds(now.0.saturating_sub(RECENT_WINDOW)),
            kinds: Vec::new(),
            trust: TrustFilter::Any,
            limit: RECENT_LIMIT,
            bodies: BodyMode::Json,
        };
        match self
            .intents
            .session_recall(task.session.clone(), RecallAsk::Episodes(query))
            .await
        {
            Ok(RecallView::Episodes(lines)) => lines,
            _ => Vec::new(),
        }
    }

    /// Automatic recall: the hits that bear on what the person said, trusted ones only and cut
    /// to the budget.
    async fn recall_for(&self, task: &OpenTask) -> Vec<RecalledLine> {
        let words = words_of(task);
        if words.is_empty() {
            return Vec::new();
        }
        let inject = InjectQuery {
            space: self.space.clone(),
            text: UserText::new(words),
            budget: self.config.assembler.recall,
            k: RECALL_K,
            over: RecallOver::Both,
            trust: TrustFilter::TrustedOnly,
        };
        match self
            .intents
            .session_recall(task.session.clone(), RecallAsk::Inject(inject))
            .await
        {
            Ok(RecallView::Hits(hits)) => hits,
            _ => Vec::new(),
        }
    }
}
