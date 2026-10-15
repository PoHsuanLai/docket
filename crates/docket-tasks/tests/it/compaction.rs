//! A session compacted and restored: the planner is shown the same view, and the summary keeps
//! the taint it was written under.

use agent_loop::{Sources, assemble};
use docket_core::{
    ActionRef, AgentConfig, CallId, Covers, LabelText, StepEnd, StepLine, StepShown, Summary,
    SummaryText, TurnId, TurnSource, TurnVia, UserTurn,
};
use docket_session::{
    BackendKind, Compacted, CompactedBy, Logged, Opening, Read, Seq, SessionEntry, Taint,
    TaintCause, TaintNote, resume_plan,
};
use docket_tasks::nowhere;
use porter_core::{AppName, Count, Tier};
use prov::{ActionName, AgentRef, Effect, Integrity, SpaceId, TaskId, UnixSeconds};

fn shell() -> AppName {
    AppName::parse("org.quire.Shell").expect("app")
}

fn opening() -> Opening {
    Opening {
        task: TaskId::parse("t-1").expect("task"),
        space: SpaceId::parse("work").expect("space"),
        opener: Some(shell()),
        agent: Some(AgentRef::Companion),
        backend: BackendKind::Native,
        parent: None,
        forked_from: None,
        cwd: None,
        started_from: None,
        label: None,
        rewind: Default::default(),
    }
}

fn turn() -> UserTurn {
    UserTurn {
        id: TurnId(1),
        text: "archive the old items".into(),
        at: UnixSeconds(1),
        from: TurnSource::Launcher,
        via: TurnVia::Typed,
    }
}

fn step(n: u64) -> StepLine {
    StepLine {
        call: CallId(n),
        action: ActionRef {
            app: shell(),
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

fn sources(history: Vec<StepLine>, taint: Integrity, summary: Option<Summary>) -> Sources {
    Sources {
        cards: vec![],
        profile: vec![],
        primer: None,
        rollup: None,
        roster: docket_core::Roster::default(),
        episodes: vec![],
        recalled: vec![],
        context: nowhere(shell()),
        turns: vec![turn()],
        history,
        handles: vec![],
        inbox: vec![],
        taint,
        task_policy: None,
        skills: vec![],
        skill_texts: vec![],
        summary,
    }
}

fn rows(entries: Vec<SessionEntry>) -> Vec<Logged> {
    entries
        .into_iter()
        .enumerate()
        .map(|(i, e)| Logged {
            seq: Seq(i as u64),
            read: Read::Entry(Box::new(e)),
        })
        .collect()
}

#[test]
fn a_restore_after_compaction_shows_the_planner_the_same_view_and_keeps_the_taint() {
    let budget = AgentConfig::default().assembler;
    let covers = Covers {
        turns: Count(0),
        steps: Count(30),
    };
    let text = SummaryText::parse("archived the first thirty items", &[]).expect("summary");
    for (written, integrity) in [
        (Taint::Clean, Integrity::Trusted),
        (Taint::Tainted, Integrity::Untrusted),
    ] {
        let mut entries = vec![SessionEntry::Opened(opening())];
        if written == Taint::Tainted {
            entries.push(SessionEntry::Taint(TaintNote {
                cause: TaintCause::UntrustedReveal,
                at_call: None,
            }));
        }
        entries.push(SessionEntry::Turn(turn()));
        entries.extend((1..=40).map(|n| SessionEntry::Step(step(n))));
        entries.push(SessionEntry::Compacted(Compacted {
            covers,
            text: text.clone(),
            taint: written,
            by: CompactedBy {
                tier: Tier::Balanced,
                model: None,
            },
        }));
        let plan = resume_plan(&rows(entries)).expect("plan");
        assert!(plan.faults.is_empty(), "{written:?}: {:?}", plan.faults);
        assert_eq!(plan.history.len(), 40, "the log still holds every step");
        assert_eq!(plan.live_history().len(), 10);

        let before = sources(
            (1..=40).map(step).collect(),
            integrity,
            Some(Summary {
                covers,
                text: text.clone(),
                integrity,
            }),
        );
        let after = sources(plan.history.clone(), integrity, plan.summary.clone());
        let (seen, restored) = (assemble(&budget, &before), assemble(&budget, &after));
        assert_eq!(seen, restored, "{written:?}");
        assert_eq!(restored.history.len(), 10);
        assert_eq!(restored.summary.map(|s| s.integrity), Some(integrity));
    }
}
