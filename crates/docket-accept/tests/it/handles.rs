//! What the planner is shown of the things a search found, and what it is told when a reply of
//! its cannot be used, through the real daemons (cassettes `flow-a-handles`, `flow-a-fault-line`,
//! `flow-a-unread`). A cassette entry that needs words in the planner's request answers nothing
//! when they are missing, so a view without the handle lines ends the answer Failed.

use crate::support::*;
use companion_wire::{AnswerPhase, AnswerWire};
use docket_accept::confirm::Verdict;
use docket_accept::drive::Launcher;
use docket_accept::world::{Cassette, Consent, World};

fn settled(view: &AnswerWire) -> bool {
    matches!(
        view.phase,
        AnswerPhase::Done | AnswerPhase::Failed | AnswerPhase::Cancelled
    )
}

/// Plays "forward the Lisbon receipts to accounting" over `cassette` and checks the two threads
/// were forwarded to accounting, held for one sheet.
async fn forwarded_over(cassette: Cassette) {
    let world = World::start(&binaries(), Consent::Standing, cassette).await;
    world.sheet.will(Verdict::Allow);
    let launcher = Launcher::of(&world).await;
    let opened = launcher.open().await;
    let mut answer = launcher
        .say(&opened, "forward the Lisbon receipts to accounting")
        .await;
    let history = answer.history_until(settled).await;
    let last = history.last().expect("a view").clone();
    if last.phase != AnswerPhase::Done {
        fail(
            &world,
            &format!("the answer ended {:?}\n{history:#?}", last.phase),
        );
    }
    let messages = world.mail.messages();
    assert_eq!(messages.len(), 1, "{messages:#?}");
    assert_eq!(messages[0].to, "accounting");
    assert_eq!(messages[0].threads, ["lisbon-1", "lisbon-2"]);
    assert_eq!(world.sheet.shown().len(), 1, "one sheet for one forward");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_planner_forwards_by_the_handles_the_searches_gave_it() {
    forwarded_over(FLOW_A_HANDLES).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unknown_handle_is_told_with_the_handles_held_and_the_planner_corrects_itself() {
    forwarded_over(FLOW_A_FAULT_LINE).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reply_that_could_not_be_read_is_told_and_the_planner_corrects_itself() {
    forwarded_over(FLOW_A_UNREAD).await;
}
