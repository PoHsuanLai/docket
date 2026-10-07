//! The restart rebuild.

use crate::support::*;
use agent_loop::*;
use almanac_core::{EpisodeKind, EpisodeOutcome};
use companion_wire::{AnswerPhase, SessionRecord};
use docket_core::*;
use porter_core::AppName;
use prov::{
    Address, AgentRef, Message, MessageId, MessageKind, MessageText, Part, ReportStatus, RunId,
    ThreadId,
};

fn message(from: AgentRef, to: AgentRef, kind: MessageKind, text: &str, in_space: &str) -> Message {
    Message {
        id: MessageId::parse("m-1").expect("id"),
        thread: ThreadId::parse("m-1").expect("thread"),
        in_reply_to: None,
        from: Address::new(from, space(in_space)),
        to: Address::new(to, space(in_space)),
        kind,
        parts: vec![Part::Text(MessageText::new(text))],
        label: prov::Label {
            integrity: prov::Integrity::Trusted,
            confidentiality: prov::Confidentiality::Public,
            classes: Default::default(),
            sources: Default::default(),
        },
        sent: at(0),
    }
}

#[test]
fn a_restart_rebuilds_the_roster_and_the_front_task_from_the_stored_records() {
    let opened = |t: &str, agent: AgentRef, parent: Option<&str>| ReplayEvent {
        at: at(1),
        what: session(SessionRecord::Opened {
            task: task(t),
            space: space("work"),
            agent,
            parent: parent.map(task),
        }),
    };
    let events = vec![
        opened("t-1", AgentRef::Companion, None),
        opened("t-2", worker("t-2"), Some("t-1")),
        ReplayEvent {
            at: at(2),
            what: session(SessionRecord::Asked {
                turn: turn(1, "archive the newsletters", 2),
                to: AgentRef::Companion,
                task: task("t-1"),
            }),
        },
        ReplayEvent {
            at: at(3),
            what: ReplayWhat::Message(Box::new(message(
                AgentRef::User,
                worker("t-2"),
                MessageKind::Request,
                "skip the newsletter folder",
                "work",
            ))),
        },
        ReplayEvent {
            at: at(4),
            what: ReplayWhat::Run {
                run: RunId::parse("r-7").expect("run"),
                space: space("home"),
                state: RosterState::Working,
            },
        },
        opened("t-3", worker("t-3"), None),
        ReplayEvent {
            at: at(5),
            what: ReplayWhat::Message(Box::new(message(
                worker("t-3"),
                AgentRef::Companion,
                MessageKind::Report {
                    status: ReportStatus::Done,
                },
                "done",
                "work",
            ))),
        },
    ];
    let rebuilt = rebuild(&events);
    assert_eq!(rebuilt.front, Some(task("t-1")));
    let roster = rebuilt.roster();
    let agents: Vec<&AgentRef> = roster.entries.iter().map(|l| &l.agent).collect();
    assert_eq!(
        agents.len(),
        3,
        "the front, one worker and one run are live; the reporting worker is done"
    );
    assert!(!agents.contains(&&worker("t-3")));
    let w2 = roster
        .entries
        .iter()
        .find(|l| l.agent == worker("t-2"))
        .expect("t-2");
    assert!(
        matches!(&w2.detail, RosterDetail::Full(f) if f.told.as_ref().map(LeadText::as_str) == Some("skip the newsletter folder"))
    );
    let run = roster
        .entries
        .iter()
        .find(|l| matches!(l.agent, AgentRef::Cua { .. }))
        .expect("run");
    assert_eq!(run.detail, RosterDetail::PresenceOnly);
    let seen = roster.seen_from(&space("work"));
    let in_home = seen
        .entries
        .iter()
        .find(|l| matches!(l.agent, AgentRef::Cua { .. }))
        .expect("run");
    assert_eq!(in_home.detail, RosterDetail::PresenceOnly);
}

#[test]
fn finishing_the_front_task_and_closing_episodes_end_their_roster_lines() {
    let opened = ReplayEvent {
        at: at(1),
        what: session(SessionRecord::Opened {
            task: task("t-1"),
            space: space("work"),
            agent: AgentRef::Companion,
            parent: None,
        }),
    };
    let finished = ReplayEvent {
        at: at(2),
        what: session(SessionRecord::Finished {
            task: task("t-1"),
            phase: AnswerPhase::Done,
        }),
    };
    let rebuilt = rebuild(&[opened.clone(), finished]);
    assert_eq!(rebuilt.front, None);
    assert!(rebuilt.roster().entries.is_empty());
    let ledger = TaskLedger {
        task: task("t-2"),
        agent: worker("t-2"),
        parent: None,
        space: space("work"),
        started: at(0),
        asked: vec![],
        steps: vec![],
        touched: vec![],
        results: vec![],
    };
    let episode =
        close(&ledger, EpisodeKind::Task, at(9), EpisodeOutcome::Failed).expect("episode");
    let opened2 = ReplayEvent {
        at: at(1),
        what: session(SessionRecord::Opened {
            task: task("t-2"),
            space: space("work"),
            agent: worker("t-2"),
            parent: None,
        }),
    };
    let rebuilt = rebuild(&[
        opened2,
        ReplayEvent {
            at: at(9),
            what: ReplayWhat::Episode(Box::new(episode)),
        },
    ]);
    assert_eq!(rebuilt.tasks[0].state, RosterState::Failed);
    let _ = AppName::parse("org.quire.Mail");
}
