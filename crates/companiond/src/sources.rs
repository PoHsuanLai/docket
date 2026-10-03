//! What the planner is shown, gathered for one turn: the working set. The router reads memory
//! for the companion (`Session.Recall`) and labels everything it returns, so an untrusted text is
//! already a handle by the time it is here. The sections are the assembler's (`agent-loop`); this
//! only fills them, and the assembler cuts each to its budget.

use crate::runtime::Companiond;
use crate::task::TaskRuntime;
use agent_loop::Sources;
use almanac_core::{
    BodyMode, InjectQuery, KindPattern, RecallOver, RecentQuery, TrustFilter, UserText,
};
use docket_client::Transport as IntentsTransport;
use docket_core::{
    ContextView, EntityLine, HereView, RecallAsk, RecallView, RecalledLine, Reveal, SelectionView,
    TextTargetView, VisibleView,
};
use porter_client::Transport as InferTransport;
use porter_core::{Count, UnixSeconds};
use prov::TaskId;

/// How many hits the automatic recall asks for before the token budget cuts them.
const RECALL_K: Count = Count(8);
/// How many recent events a turn reads.
const RECENT_LIMIT: Count = Count(20);
/// How far back recent activity reaches: a day.
const RECENT_WINDOW: i64 = 24 * 3600;

/// The view of where the person is when no app was named: nowhere in particular.
fn nowhere(app: porter_core::AppName) -> ContextView {
    ContextView {
        app,
        window: Reveal::Plain(String::new()),
        here: HereView::Nowhere,
        selection: SelectionView::Nothing,
        visible: VisibleView {
            kind: None,
            items: Vec::<EntityLine>::new(),
            total: Count(0),
        },
        text_target: TextTargetView::None,
    }
}

fn words_of(rt: &TaskRuntime) -> String {
    rt.turns
        .iter()
        .map(|t| t.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

impl<P: InferTransport, I: IntentsTransport> Companiond<P, I> {
    /// The sections for one planner turn of `task`. A source that does not answer leaves its
    /// section empty: the planner works with less, never with a guess.
    pub(crate) async fn sources(&mut self, task: &TaskId) -> Sources {
        // What landed since the last turn is part of this one.
        let _ = self.pull().await;
        let Some(rt) = self.runtimes.get(task).cloned() else {
            return self.empty_sources();
        };
        let now = self.clock.now();
        let context = self.context_of(&rt).await;
        let recalled = self.recall_for(&rt, now).await;
        let task_policy = self
            .intents
            .session_task_policy(rt.session.clone())
            .await
            .unwrap_or_default();
        let roster = self.roster_without(Some(task)).seen_from(&rt.space);
        let episodes = self
            .episodes
            .iter()
            .filter(|e| e.space == rt.space)
            .cloned()
            .collect();
        Sources {
            cards: self.planner.catalogue().cards(),
            profile: Vec::new(),
            primer: None,
            rollup: None,
            roster,
            episodes,
            recalled,
            context,
            turns: rt.turns.clone(),
            history: rt.history.clone(),
            handles: rt.handles.clone(),
            inbox: rt.inbox.clone(),
            taint: rt.taint(),
            task_policy,
        }
    }

    fn empty_sources(&self) -> Sources {
        Sources {
            cards: Vec::new(),
            profile: Vec::new(),
            primer: None,
            rollup: None,
            roster: docket_core::Roster::default(),
            episodes: Vec::new(),
            recalled: Vec::new(),
            context: nowhere(self.shell.clone()),
            turns: Vec::new(),
            history: Vec::new(),
            handles: Vec::new(),
            inbox: Vec::new(),
            taint: prov::Integrity::Trusted,
            task_policy: None,
        }
    }

    /// Where the person is: the window of the app they summoned the companion from, or nowhere
    /// in particular when it was the launcher alone.
    async fn context_of(&self, rt: &TaskRuntime) -> ContextView {
        match &self.summoned {
            Some(app) => self
                .intents
                .context(rt.session.clone(), app.clone())
                .await
                .unwrap_or_else(|_| nowhere(app.clone())),
            None => nowhere(self.shell.clone()),
        }
    }

    /// Automatic recall (Q3): the hits that bear on what the person said, trusted ones only and
    /// cut to the budget, and what happened lately in the Space before this run began (this run's
    /// own episodes are already in the episodes section).
    async fn recall_for(&self, rt: &TaskRuntime, now: UnixSeconds) -> Vec<RecalledLine> {
        let mut lines = Vec::new();
        let words = words_of(rt);
        if !words.is_empty() {
            let inject = InjectQuery {
                space: rt.space.clone(),
                text: UserText::new(words),
                budget: self.config.assembler.recall,
                k: RECALL_K,
                over: RecallOver::Both,
                trust: TrustFilter::TrustedOnly,
            };
            if let Ok(RecallView::Hits(hits)) = self
                .intents
                .session_recall(rt.session.clone(), RecallAsk::Inject(inject))
                .await
            {
                lines.extend(hits);
            }
        }
        let episodes = KindPattern::parse("companion.episode")
            .ok()
            .into_iter()
            .collect();
        let recent = RecentQuery {
            since: UnixSeconds(now.0.saturating_sub(RECENT_WINDOW)),
            kinds: episodes,
            trust: TrustFilter::Any,
            limit: RECENT_LIMIT,
            bodies: BodyMode::Without,
        };
        if let Ok(RecallView::Recent(events)) = self
            .intents
            .session_recall(rt.session.clone(), RecallAsk::Recent(recent))
            .await
        {
            let before = self.booted;
            lines.extend(events.into_iter().enumerate().filter_map(|(i, e)| {
                let text = e.text?;
                (e.summary.occurred < before).then(|| RecalledLine {
                    doc: almanac_core::MemoryItem::Event(e.summary.event),
                    at: e.summary.occurred,
                    text,
                    // Newest first: the rank is the recency order.
                    why: almanac_core::RecallWhy::Lexical {
                        rank: u32::try_from(i + 1).unwrap_or(u32::MAX),
                    },
                })
            }));
        }
        lines
    }
}
