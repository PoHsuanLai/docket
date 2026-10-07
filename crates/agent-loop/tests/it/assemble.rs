//! The assembler: budgets per section, masking, and a byte-stable prefix.

use agent_loop::*;
use almanac_core::{MemoryItem, RecallWhy};
use docket_core::*;
use porter_core::{AppName, Count, Tokens};
use prov::{ActionName, Effect, Integrity, SpaceId, UnixSeconds};

fn app() -> AppName {
    AppName::parse("org.quire.Mail").expect("app")
}

fn card(name: &str) -> ActionCard {
    ActionCard {
        action: ActionRef {
            app: app(),
            name: ActionName::parse(name).expect("action"),
        },
        label: LabelText::parse("Do it").expect("label"),
        effect: Effect::Read,
        on: TargetKind::Nothing,
        tool: ToolSchema(serde_json::json!({"type": "object"})),
        reach: AgentReach::Offered,
        lasting: Lasting::No,
    }
}

fn context() -> ContextView {
    ContextView {
        app: app(),
        window: Reveal::Plain("Inbox".into()),
        here: HereView::Nowhere,
        selection: SelectionView::Nothing,
        visible: VisibleView {
            kind: None,
            items: vec![],
            total: Count(0),
        },
        text_target: TextTargetView::None,
    }
}

fn step(n: u64, undo: Option<u64>) -> StepLine {
    StepLine {
        call: CallId(n),
        action: ActionRef {
            app: app(),
            name: ActionName::parse("mail.thread.archive").expect("action"),
        },
        effect: Effect::UndoableWrite,
        end: StepEnd::Done {
            said: Some(LabelText::parse("Archived 12 messages").expect("said")),
            value: Some(Reveal::Plain(Value::Integer(12))),
            undo: undo.map(UndoId),
        },
        shown: StepShown::Full,
        with: vec![],
    }
}

fn recalled(rank: u32, text: &str) -> RecalledLine {
    RecalledLine {
        doc: MemoryItem::Fact(almanac_core::FactId::mint(1_700_000_000_000, [7; 10])),
        at: UnixSeconds(1),
        text: Reveal::Plain(text.into()),
        why: RecallWhy::Lexical { rank },
    }
}

fn sources() -> Sources {
    Sources {
        cards: vec![card("mail.thread.read"), card("mail.thread.archive")],
        profile: vec![ProfileLine("name: Po".into())],
        primer: Some(PrimerText("likes short answers".into())),
        rollup: None,
        roster: Roster::default(),
        episodes: vec![],
        recalled: vec![],
        context: context(),
        turns: vec![UserTurn {
            id: TurnId(1),
            text: "archive the newsletters".into(),
            at: UnixSeconds(5),
            from: TurnSource::Launcher,
            via: TurnVia::Typed,
        }],
        history: vec![],
        handles: vec![],
        inbox: vec![],
        skills: vec![],
        skill_texts: vec![],
        taint: Integrity::Trusted,
        task_policy: None,
    }
}

fn budget() -> AssemblerBudget {
    AgentConfig::default().assembler
}

#[test]
fn everything_inside_the_budget_passes_through_unchanged() {
    let s = sources();
    let view = assemble(&budget(), &s);
    assert_eq!(view.actions, s.cards);
    assert_eq!(view.turns, s.turns);
    assert_eq!(view.primer, s.primer);
    assert_eq!(view.profile, s.profile);
    assert_eq!(view.taint, Integrity::Trusted);
    assert_eq!(task_fit(&budget(), &view), TaskFit::Fits);
}

#[test]
fn only_the_newest_steps_stay_in_full() {
    let history: Vec<StepLine> = (1..=5).map(|n| step(n, Some(40 + n))).collect();
    let masked = mask_history(&history, Count(3));
    let shown: Vec<StepShown> = masked.iter().map(|s| s.shown).collect();
    assert_eq!(
        shown,
        [
            StepShown::Masked,
            StepShown::Masked,
            StepShown::Full,
            StepShown::Full,
            StepShown::Full
        ]
    );
    let StepEnd::Done { said, value, undo } = &masked[0].end else {
        panic!("done")
    };
    assert_eq!(value, &None, "the value is gone");
    assert!(
        said.is_some() && *undo == Some(UndoId(41)),
        "the line keeps what the app said and the undo row"
    );
    assert_eq!(masked[4], history[4]);
    assert_eq!(
        mask_history(&history[..2], Count(3)),
        history[..2].to_vec(),
        "fewer steps than the window are untouched"
    );
}

#[test]
fn masking_keeps_the_handles_a_step_named_and_returned_and_drops_other_values() {
    let mut read = step(1, None);
    read.with = vec![Handle(1)];
    read.end = StepEnd::Done {
        said: None,
        value: Some(Reveal::Handle(Handle(4))),
        undo: None,
    };
    let mut search = step(2, None);
    search.end = StepEnd::Done {
        said: None,
        value: Some(Reveal::Plain(Value::List(vec![Value::Handle(Handle(1))]))),
        undo: None,
    };
    let plain = step(3, None);
    let history = vec![read.clone(), search.clone(), plain, step(4, None)];
    let masked = mask_history(&history, Count(1));
    assert_eq!(masked[0].with, vec![Handle(1)]);
    assert_eq!(masked[0].end, read.end);
    assert_eq!(masked[1].end, search.end);
    let StepEnd::Done { value, .. } = &masked[2].end else {
        panic!("done")
    };
    assert_eq!(value, &None, "a count is not a handle");
}

#[test]
fn the_oldest_masked_steps_go_first_when_the_task_is_over_budget_and_the_turns_never_do() {
    let mut s = sources();
    s.history = (1..=40).map(|n| step(n, Some(n))).collect();
    let mut tight = budget();
    tight.task = Tokens(400);
    let view = assemble(&tight, &s);
    assert!(
        view.history.len() < 40 && view.history.len() >= 3,
        "{}",
        view.history.len()
    );
    assert_eq!(
        view.history.last().map(|h| h.call),
        Some(CallId(40)),
        "the newest stays"
    );
    assert_eq!(view.turns, s.turns);
    let roomy = assemble(&budget(), &s);
    assert_eq!(
        roomy.history.len(),
        40,
        "with room nothing is dropped, only masked"
    );
    let mut tiny = budget();
    tiny.task = Tokens(1);
    let view = assemble(&tiny, &s);
    assert_eq!(
        view.history.len(),
        3,
        "the newest three stay in full even over budget"
    );
    assert_eq!(
        task_fit(&tiny, &view),
        TaskFit::Overflows,
        "the loop splits the task"
    );
}

#[test]
fn recall_keeps_ranked_order_and_skips_what_does_not_fit() {
    let mut s = sources();
    let big = "x".repeat(8000);
    s.recalled = vec![
        recalled(1, "short one"),
        recalled(2, &big),
        recalled(3, "short two"),
    ];
    let view = assemble(&budget(), &s);
    let texts: Vec<String> = view
        .recalled
        .iter()
        .map(|r| match &r.text {
            Reveal::Plain(t) => t.clone(),
            Reveal::Handle(_) => String::new(),
        })
        .collect();
    assert_eq!(texts, ["short one", "short two"]);
}

#[test]
fn profile_roster_and_episodes_are_cut_in_order_at_their_budgets() {
    let mut s = sources();
    s.profile = (0..200)
        .map(|n| ProfileLine(format!("fact number {n} about the person")))
        .collect();
    let mut b = budget();
    b.profile = Tokens(60);
    let view = assemble(&b, &s);
    assert!(!view.profile.is_empty() && view.profile.len() < 200);
    assert_eq!(
        view.profile[0], s.profile[0],
        "the cut is a prefix, in order"
    );
    assert_eq!(
        view.primer, None,
        "the primer drops when the profile used the room"
    );
}

#[test]
fn a_long_visible_list_is_shortened_to_the_context_budget() {
    let mut s = sources();
    let thread = |n: u32| EntityLine {
        id: prov::EntityId {
            app: app(),
            kind: prov::EntityKind::parse("mail.thread").expect("kind"),
            key: prov::EntityKey::parse(&format!("t{n}")).expect("key"),
        },
        title: Reveal::Handle(Handle(u64::from(n))),
        subtitle: Reveal::Handle(Handle(u64::from(n) + 1000)),
    };
    s.context.visible = VisibleView {
        kind: None,
        items: (0..300).map(thread).collect(),
        total: Count(300),
    };
    let view = assemble(&budget(), &s);
    assert!(view.context.visible.items.len() < 300);
    assert_eq!(
        view.context.visible.total,
        Count(300),
        "the total still says how many there were"
    );
}

#[test]
fn the_stable_sections_do_not_change_between_turns() {
    let a = sources();
    let mut b = sources();
    b.turns.push(UserTurn {
        id: TurnId(2),
        text: "and the receipts".into(),
        at: UnixSeconds(99),
        from: TurnSource::Launcher,
        via: TurnVia::Typed,
    });
    b.history = vec![step(1, None)];
    b.context.window = Reveal::Plain("Another window".into());
    let (va, vb) = (assemble(&budget(), &a), assemble(&budget(), &b));
    let prefix = |v: &PlannerView| {
        serde_json::to_string(&(&v.actions, &v.primer, &v.profile, &v.rollup)).expect("json")
    };
    assert_eq!(
        prefix(&va),
        prefix(&vb),
        "rules, cards, profile and primer are byte-stable"
    );
    assert_eq!(
        serde_json::to_string(&assemble(&budget(), &a)).expect("json"),
        serde_json::to_string(&va).expect("json"),
        "assembling twice is deterministic"
    );
    let _ = SpaceId::parse("work");
}

#[test]
fn a_refusal_tells_the_planner_only_the_coarse_code_and_a_trip_pauses_the_loop() {
    let idle = LoopState {
        phase: LoopPhase::AwaitingCalls,
        turn: Some(TurnId(1)),
        steps: 1,
        pending: vec![CallId(1)],
        guard: Default::default(),
    };
    let refused = StepEnd::Refused(CallRefusal::Denied(DenyCode::OutsideTask));
    let (state, effects) = agent_step(idle.clone(), LoopInput::CallEnded(CallId(1), refused));
    assert_eq!(state.phase, LoopPhase::Planning);
    assert!(effects.contains(&LoopEffect::AskPlanner));
    let (state, _) = agent_step(idle, LoopInput::Tripped(BreakerTrip::Probing));
    assert_eq!(state.phase, LoopPhase::Paused(BreakerTrip::Probing));
}

#[test]
fn assembled_view_keeps_untrusted_text_behind_handles() {
    let mut s = sources();
    let mut line = recalled(1, "unused");
    line.text = Reveal::Handle(Handle(4));
    s.recalled = vec![line];
    let mut done = step(1, Some(3));
    done.end = StepEnd::Done {
        said: None,
        value: Some(Reveal::Handle(Handle(5))),
        undo: None,
    };
    s.history = vec![done];
    s.handles = vec![HandleCard {
        handle: Handle(4),
        shape: HandleShape::Text,
        from: prov::Source::Mail,
        size: CharCount(40),
    }];
    let view = assemble(&budget(), &s);
    assert_eq!(
        view.recalled[0].text,
        Reveal::Handle(Handle(4)),
        "a handle stays a handle"
    );
    let json = serde_json::to_string(&view).expect("json");
    for secret in ["Ignore previous", "wire the money", "evil.test"] {
        assert!(!json.contains(secret), "{secret} reached the view");
    }
    assert!(
        json.contains("archive the newsletters"),
        "the person's own words stay plain"
    );
}
