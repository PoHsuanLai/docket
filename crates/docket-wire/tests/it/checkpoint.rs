//! The checkpoint requests and replies of the wire.

use companion_model::{
    AppSessionId, HandNote, HandTo, WatchOwner, WatchedAgent, WatchedLine, Watching,
};
use docket_core::{
    CheckpointFault, CheckpointId, CheckpointList, RestorePlan, Retention, Rewind, TurnEnd,
    TurnState,
};
use docket_permissions_core::ProgramName;
use docket_vocab::{Days, TurnId, Workspace};
use docket_wire::{IntentsReply, IntentsRequest, Member};
use porter_core::Count;
use prov::SessionId;
use serde::{Serialize, de::DeserializeOwned};

fn round<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: &T) -> String {
    let json = serde_json::to_string(value).expect("json");
    let back: T = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{json}: {e}"));
    assert_eq!(&back, value, "{json}");
    json
}

#[test]
fn the_checkpoint_requests_and_replies_round_trip_and_name_their_members() {
    let session = SessionId::parse("s-1").expect("session");
    let list = IntentsRequest::CheckpointList {
        session: session.clone(),
    };
    let plan = IntentsRequest::CheckpointPlan {
        session: session.clone(),
        id: CheckpointId(2),
    };
    assert_eq!(list.member(), Member::CheckpointList);
    assert_eq!(plan.member(), Member::CheckpointPlan);
    round(&list);
    round(&plan);
    round(&IntentsReply::Checkpoints(Box::new(CheckpointList {
        session,
        rows: vec![],
        keeps: Retention {
            last: Count(20),
            days: Days(14),
        },
        turn: TurnState::Idle,
        label: None,
    })));
    let ended = IntentsRequest::SessionTurnEnded {
        session: SessionId::parse("s-1").expect("session"),
        turn: TurnId(3),
        how: TurnEnd::Cancelled,
    };
    assert_eq!(ended.member(), Member::SessionTurnEnded);
    round(&ended);
    let watch = IntentsRequest::CheckpointWatch {
        workspace: Workspace::parse("/work/project").expect("workspace"),
        label: "claude in pane 2".into(),
        rewind: Rewind::Agent,
    };
    let mark = IntentsRequest::CheckpointMark {
        session: SessionId::parse("s-2").expect("session"),
    };
    assert_eq!(watch.member(), Member::CheckpointWatch);
    assert_eq!(mark.member(), Member::CheckpointMark);
    round(&watch);
    round(&mark);
    round(&IntentsReply::CheckpointWatching(
        SessionId::parse("s-2").expect("session"),
    ));
    round(&IntentsReply::CheckpointPlan(Ok(RestorePlan::default())));
    round(&IntentsReply::CheckpointPlan(Err(CheckpointFault::Gone)));
}

#[test]
fn a_conversation_moving_between_docket_and_a_terminal_has_its_members_and_shapes() {
    let session = SessionId::parse("s-2").expect("session");
    let agent = WatchedAgent {
        program: ProgramName::parse("claude-code").expect("program"),
        session: AppSessionId::parse("0a1b-2c3d").expect("id"),
    };
    let watch = IntentsRequest::CheckpointWatchAgent {
        workspace: Workspace::parse("/work/project").expect("workspace"),
        label: "claude in pane 2".into(),
        rewind: Rewind::Agent,
        agent: agent.clone(),
    };
    let hand = IntentsRequest::CheckpointHandover {
        session: session.clone(),
        to: HandTo::Terminal,
        agent: Some(agent.clone()),
    };
    assert_eq!(watch.member(), Member::CheckpointWatchAgent);
    assert_eq!(hand.member(), Member::CheckpointHandover);
    assert_eq!(
        IntentsRequest::CheckpointWatched.member(),
        Member::CheckpointWatched
    );
    round(&watch);
    round(&hand);
    round(&IntentsRequest::CheckpointWatched);
    round(&IntentsReply::HandedOver(Some(agent.clone())));
    let handed = Watching {
        session: session.clone(),
        owner: WatchOwner::Companion,
    };
    assert!(round(&IntentsReply::CheckpointWatchingAgent(handed)).contains("companion"));
    round(&IntentsReply::Watched(vec![WatchedLine {
        session,
        agent,
        label: None,
        folder: Some(Workspace::parse("/work/project").expect("workspace")),
    }]));
    // The log's note reads without the agent when an older writer left it out.
    let note: HandNote = serde_json::from_str(r#"{"to":"desktop"}"#).expect("note");
    assert_eq!(
        note,
        HandNote {
            to: HandTo::Desktop,
            agent: None
        }
    );
}
