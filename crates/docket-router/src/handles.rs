//! The handle table and the planner's view. When the router hands anything to a companion
//! session (context, search hits, read outcomes, previews), every untrusted text is replaced by
//! a handle; the text reaches only the reader, and the screen through `Session.Display`. A
//! model cannot forge taint: the router computes it.

use docket_core::{
    ActionCard, ContextSnapshot, ContextView, EntityLine, EntityRef, EpisodeLine, FileRef, Handle,
    HandleCard, HandleShape, HereView, InboundLine, PlannerView, PrimerText, ProfileLine,
    RecalledLine, Reveal, RollupLine, Roster, SelectionView, StepLine, TaskPolicy, TextTargetView,
    UserTurn, VisibleView, WindowPrivacy, size_of,
};
use docket_core::{Here, Selection, TextTarget};
use prov::{EntityId, Integrity, Label, Labelled, Measured, Quarantined, Source};
use std::collections::BTreeMap;

/// What a handle holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandleValue {
    /// Words.
    Text(String),
    /// A thing.
    Entity(EntityId),
    /// A file.
    File(FileRef),
}

impl Measured for HandleValue {
    fn measure(&self) -> usize {
        match self {
            HandleValue::Text(t) => t.len(),
            HandleValue::Entity(_) | HandleValue::File(_) => 1,
        }
    }
}

/// One held value with its label and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandleEntry {
    /// The value and its label.
    pub value: Labelled<HandleValue>,
    /// Where it came from.
    pub from: Source,
}

/// The values a session holds for readers. Router-owned; one per session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HandleTable {
    next: u64,
    entries: BTreeMap<Handle, HandleEntry>,
}

impl HandleTable {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Holds a value and returns its handle.
    pub fn mint(&mut self, value: Labelled<HandleValue>, from: Source) -> Handle {
        self.next += 1;
        let handle = Handle(self.next);
        self.entries.insert(handle, HandleEntry { value, from });
        handle
    }

    /// Words for a planner: plain when trusted, a handle when not.
    pub fn reveal(&mut self, text: Labelled<String>, from: Source) -> Reveal<String> {
        match text.label.integrity {
            Integrity::Trusted => Reveal::Plain(text.value),
            Integrity::Untrusted => Reveal::Handle(self.mint(
                Labelled {
                    value: HandleValue::Text(text.value),
                    label: text.label,
                },
                from,
            )),
        }
    }

    /// What a planner is told about a handle: its shape, source and size, never its content.
    pub fn card(&self, handle: Handle) -> Option<HandleCard> {
        let entry = self.entries.get(&handle)?;
        let (shape, size) = match &entry.value.value {
            HandleValue::Text(t) => (HandleShape::Text, size_of(t)),
            HandleValue::Entity(e) => (
                HandleShape::Entity(e.kind.clone()),
                docket_core::CharCount(0),
            ),
            HandleValue::File(_) => (HandleShape::File, docket_core::CharCount(0)),
        };
        Some(HandleCard {
            handle,
            shape,
            from: entry.from.clone(),
            size,
        })
    }

    /// Cards for every handle, in order.
    pub fn cards(&self) -> Vec<HandleCard> {
        self.entries.keys().filter_map(|h| self.card(*h)).collect()
    }

    /// What a handle holds, with its label, for the router to put back in an argument.
    pub fn value(&self, handle: Handle) -> Option<&Labelled<HandleValue>> {
        self.entries.get(&handle).map(|e| &e.value)
    }

    /// A handle's label.
    pub fn label(&self, handle: Handle) -> Option<&Label> {
        self.entries.get(&handle).map(|e| &e.value.label)
    }

    /// A text handle's content for the reader (`Session.Resolve`, the reader role only), still
    /// quarantined: only the reader's host holds the key that opens it.
    pub fn resolve_text(&self, handle: Handle) -> Option<Quarantined<String>> {
        let entry = self.entries.get(&handle)?;
        match &entry.value.value {
            HandleValue::Text(t) => Some(Quarantined::new(Labelled {
                value: t.clone(),
                label: entry.value.label.clone(),
            })),
            HandleValue::Entity(_) | HandleValue::File(_) => None,
        }
    }

    /// A text handle's content for the screen (`Session.Display`, never for a model).
    pub fn display(&self, handle: Handle) -> Option<&str> {
        match &self.entries.get(&handle)?.value.value {
            HandleValue::Text(t) => Some(t),
            HandleValue::Entity(_) | HandleValue::File(_) => None,
        }
    }
}

fn source_of(label: &Label, fallback: &Source) -> Source {
    label
        .sources
        .iter()
        .next()
        .cloned()
        .unwrap_or_else(|| fallback.clone())
}

fn line(table: &mut HandleTable, e: &EntityRef, fallback: &Source) -> EntityLine {
    EntityLine {
        id: e.id.clone(),
        title: table.reveal(e.title.clone(), source_of(&e.title.label, fallback)),
        subtitle: table.reveal(e.subtitle.clone(), source_of(&e.subtitle.label, fallback)),
    }
}

/// A snapshot as a planner may see it: every untrusted text a handle. A private window shows
/// the app and nothing else.
pub fn context_view(ctx: &ContextSnapshot, table: &mut HandleTable) -> ContextView {
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
                total: porter_core::Count(0),
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
    }
}
