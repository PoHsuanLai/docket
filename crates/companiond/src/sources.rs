//! What the planner is shown, gathered for one turn: the working set. The router reads memory
//! for the companion (`Session.Recall`) and labels everything it returns, so an untrusted text is
//! already a handle by the time it is here. The sections are the assembler's (`agent-loop`); this
//! only fills them, and the assembler cuts each to its budget.

use crate::runtime::Companiond;
use crate::task::TaskRuntime;
use agent_loop::Sources;
use almanac_core::{BodyMode, InjectQuery, RecallOver, RecentQuery, TrustFilter, UserText};
use docket_client::Transport as IntentsTransport;
use docket_core::{
    ActionCard, ContextView, EntityLine, EpisodeLine, HereView, PrimerText, ProfileLine, RecallAsk,
    RecallView, RecalledLine, Reveal, SelectionView, SkillCard, SkillText, TextTargetView,
    VisibleView,
};
use docket_skills::{Situation, Skill, preselect, uses_first};
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

/// What the context says for preselecting skills: the focused app and the kinds of thing in it.
fn situation(context: &ContextView) -> Situation {
    let here = match &context.here {
        HereView::Entity(line) => Some(line.id.kind.clone()),
        HereView::Nowhere | HereView::View { .. } => None,
    };
    let selected = match &context.selection {
        SelectionView::Entities { kind, .. } => Some(kind.clone()),
        _ => None,
    };
    Situation {
        focused: Some(context.app.clone()),
        kinds: here
            .into_iter()
            .chain(selected)
            .chain(context.visible.kind.clone())
            .collect(),
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
        self.refresh_handles(task).await;
        let Some(rt) = self.runtimes.get(task).cloned() else {
            return self.empty_sources();
        };
        let now = self.clock.now();
        let context = self.context_of(&rt).await;
        let recalled = self.recall_for(&rt).await;
        let task_policy = self
            .intents
            .session_task_policy(rt.session.clone())
            .await
            .unwrap_or_default();
        let roster = self.roster_without(Some(task)).seen_from(&rt.space);
        let episodes = self.episodes_for(&rt, now).await;
        let (primer, profile) = self.memory_sections(&rt).await;
        let (skills, skill_texts, cards) = self.skill_sections(&rt, &context);
        Sources {
            cards,
            profile,
            primer,
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
            skills,
            skill_texts,
        }
    }

    /// The skill catalogue, the bodies shown this turn (preselected by the context, then those the
    /// planner loaded) and the action cards with the loaded skills' actions first. A skill only
    /// reorders and explains: no card is added or removed.
    fn skill_sections(
        &self,
        rt: &TaskRuntime,
        context: &ContextView,
    ) -> (Vec<SkillCard>, Vec<SkillText>, Vec<ActionCard>) {
        let offered = self.skills.offered(&self.manifests);
        let cards = offered.iter().map(|s| s.card()).collect();
        let loaded: Vec<&Skill> = rt
            .loaded
            .iter()
            .filter_map(|id| offered.iter().copied().find(|s| &s.id == id))
            .collect();
        let shown = preselect(&offered, &situation(context));
        let texts = shown
            .into_iter()
            .chain(loaded.iter().copied())
            .fold(Vec::<&Skill>::new(), |mut acc, s| {
                if acc.iter().all(|a| a.id != s.id) {
                    acc.push(s);
                }
                acc
            })
            .into_iter()
            .map(Skill::text)
            .collect();
        let actions = uses_first(self.planner.catalogue().cards(), |c| &c.action, &loaded);
        (cards, texts, actions)
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
            skills: Vec::new(),
            skill_texts: Vec::new(),
        }
    }

    /// What the router holds by handle for the task, in shape and size: its cards replace the
    /// placeholders the task kept (the router is the one who knows how long the text is).
    async fn refresh_handles(&mut self, task: &TaskId) {
        let Some(session) = self.runtimes.get(task).map(|rt| rt.session.clone()) else {
            return;
        };
        if let Ok(cards) = self.intents.session_handles(session).await
            && let Some(rt) = self.runtimes.get_mut(task)
        {
            rt.handles = cards;
        }
    }

    /// The Space's primer and the person's own profile, which the router reads from memory: an
    /// answer that does not come leaves its section empty.
    async fn memory_sections(&self, rt: &TaskRuntime) -> (Option<PrimerText>, Vec<ProfileLine>) {
        let primer = match self
            .intents
            .session_recall(rt.session.clone(), RecallAsk::Primer)
            .await
        {
            Ok(RecallView::Primer(text)) if !text.0.is_empty() => Some(text),
            _ => None,
        };
        let profile = match self
            .intents
            .session_recall(rt.session.clone(), RecallAsk::Profile)
            .await
        {
            Ok(RecallView::Profile(lines)) => lines,
            _ => Vec::new(),
        };
        (primer, profile)
    }

    /// What recently ended in the task's Space: this run's own episodes, then what the router
    /// holds from before it began (the skeleton as trusted lines, a narrative only by handle).
    async fn episodes_for(&self, rt: &TaskRuntime, now: UnixSeconds) -> Vec<EpisodeLine> {
        let mut lines: Vec<EpisodeLine> = self
            .episodes
            .iter()
            .filter(|e| e.space == rt.space)
            .cloned()
            .collect();
        let query = RecentQuery {
            since: UnixSeconds(now.0.saturating_sub(RECENT_WINDOW)),
            kinds: Vec::new(),
            trust: TrustFilter::Any,
            limit: RECENT_LIMIT,
            bodies: BodyMode::Json,
        };
        if let Ok(RecallView::Episodes(older)) = self
            .intents
            .session_recall(rt.session.clone(), RecallAsk::Episodes(query))
            .await
        {
            let known: Vec<_> = lines.iter().map(|l| l.id.clone()).collect();
            lines.extend(
                older
                    .into_iter()
                    .filter(|l| l.ended < self.booted && !known.contains(&l.id)),
            );
        }
        lines
    }

    /// Where the person is: the window of the app they summoned the companion from, or nowhere
    /// in particular when it was the launcher alone.
    async fn context_of(&self, rt: &TaskRuntime) -> ContextView {
        match &rt.summoned {
            Some(app) => self
                .intents
                .context(rt.session.clone(), app.clone())
                .await
                .unwrap_or_else(|_| nowhere(app.clone())),
            None => nowhere(self.shell.clone()),
        }
    }

    /// Automatic recall (Q3): the hits that bear on what the person said, trusted ones only and
    /// cut to the budget.
    async fn recall_for(&self, rt: &TaskRuntime) -> Vec<RecalledLine> {
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
        lines
    }
}
