//! The planner's view: the context and everything else a planner sees, built from labelled
//! sources with every untrusted text behind a handle of the table.

use crate::handles::HandleTable;
use docket_core::{
    ActionCard, ContextSnapshot, ContextView, EntityLine, EpisodeLine, Here, HereView, InboundLine,
    PlannerView, PrimerText, ProfileLine, RecalledLine, Reveal, RollupLine, Roster, Selection,
    SelectionView, StepLine, TextTarget, TextTargetView, UserTurn, VisibleView, WindowPrivacy,
};
use docket_permissions_core::TaskPolicy;
use docket_vocab::EntityRef;
use prov::{Integrity, Label, Source};

fn source_of(label: &Label, fallback: &Source) -> Source {
    label
        .sources
        .iter()
        .next()
        .cloned()
        .unwrap_or_else(|| fallback.clone())
}

fn line(table: &mut HandleTable, e: &EntityRef, fallback: &Source) -> EntityLine {
    let title = table.reveal(e.title.clone(), source_of(&e.title.label, fallback));
    let subtitle = table.reveal(e.subtitle.clone(), source_of(&e.subtitle.label, fallback));
    EntityLine {
        // The thing's own handle carries what a `{app, kind, key}` for a shown thing carries:
        // the app's label (see `labels::trace`).
        handle: table.mint_entity(e.id.clone(), crate::labels::app_label(&e.id.app)),
        id: e.id.clone(),
        title,
        subtitle,
    }
}

/// A snapshot as a planner may see it: every untrusted text a handle. A private window shows
/// the app and nothing else.
pub fn context_view(ctx: &ContextSnapshot, table: &mut HandleTable) -> ContextView {
    table.begin_view();
    let app = Source::App(ctx.app.clone());
    if ctx.privacy == WindowPrivacy::Private {
        return ContextView {
            app: ctx.app.clone(),
            window: Reveal::Plain(String::new()),
            here: HereView::Nowhere,
            selection: SelectionView::Nothing,
            visible: VisibleView {
                kind: None,
                items: vec![],
                total: porter_ids::Count(0),
            },
            text_target: TextTargetView::None,
        };
    }
    ContextView {
        app: ctx.app.clone(),
        window: table.reveal(ctx.window.clone(), source_of(&ctx.window.label, &app)),
        here: match &ctx.here {
            Here::Nowhere => HereView::Nowhere,
            Here::Entity(e) => HereView::Entity(line(table, e, &app)),
            Here::View { view, query } => HereView::View {
                view: view.clone(),
                query: query
                    .as_ref()
                    .map(|q| table.reveal(q.clone(), source_of(&q.label, &app))),
            },
        },
        selection: match &ctx.selection {
            Selection::Nothing => SelectionView::Nothing,
            Selection::Entities { kind, items } => SelectionView::Entities {
                kind: kind.clone(),
                items: items.iter().map(|e| line(table, e, &app)).collect(),
            },
            Selection::Text(t) => {
                SelectionView::Text(table.reveal(t.clone(), source_of(&t.label, &app)))
            }
            Selection::Files(files) => SelectionView::Files(files.clone()),
        },
        visible: VisibleView {
            kind: ctx.visible.kind.clone(),
            items: ctx
                .visible
                .items
                .iter()
                .map(|e| line(table, e, &app))
                .collect(),
            total: ctx.visible.total,
        },
        text_target: match &ctx.text_target {
            TextTarget::None => TextTargetView::None,
            TextTarget::Field(f) => TextTargetView::Field {
                field: f.field.clone(),
                purpose: f.purpose,
                selection: f.selection,
                around: table.reveal(f.around.clone(), source_of(&f.around.label, &app)),
            },
        },
    }
}

/// The rest of what a planner sees, each already built from labelled sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewInputs {
    /// The person's own words.
    pub turns: Vec<UserTurn>,
    /// What the planner may call.
    pub actions: Vec<ActionCard>,
    /// The session's calls.
    pub history: Vec<StepLine>,
    /// The planner's integrity as the router computed it.
    pub taint: Integrity,
    /// What the task may do.
    pub task_policy: Option<TaskPolicy>,
    /// The Space's primer.
    pub primer: Option<PrimerText>,
    /// Pinned desktop-scope facts.
    pub profile: Vec<ProfileLine>,
    /// The latest digest line.
    pub rollup: Option<RollupLine>,
    /// Who else is working, already cut to what this Space may see.
    pub roster: Roster,
    /// Recent episodes.
    pub episodes: Vec<EpisodeLine>,
    /// What recall brought up.
    pub recalled: Vec<RecalledLine>,
    /// Messages that landed.
    pub inbox: Vec<InboundLine>,
}

/// The only builder of what a planner sees: the context as a view, the handle cards, and the
/// inputs.
pub fn planner_view(
    ctx: &ContextSnapshot,
    table: &mut HandleTable,
    inputs: ViewInputs,
) -> PlannerView {
    let context = context_view(ctx, table);
    PlannerView {
        turns: inputs.turns,
        context,
        actions: inputs.actions,
        handles: table.cards(),
        history: inputs.history,
        taint: inputs.taint,
        task_policy: inputs.task_policy,
        primer: inputs.primer,
        profile: inputs.profile,
        rollup: inputs.rollup,
        roster: inputs.roster,
        episodes: inputs.episodes,
        recalled: inputs.recalled,
        inbox: inputs.inbox,
        // Skills are companiond's: the router's view carries none.
        skills: Vec::new(),
        skill_texts: Vec::new(),
        shown: Vec::new(),
        summary: None,
    }
}
