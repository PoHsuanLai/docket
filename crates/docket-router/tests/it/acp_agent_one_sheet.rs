//! An agent that issues the same read twice at once is asked once: the second call waits on the
//! first sheet and takes its answer. Different actions never share a sheet.

use crate::acp_agent::*;
use crate::acp_agent_reads::{files_read, grants_of, open_wide};
use docket_core::*;
use docket_fake::ScriptedConfirmer;
use futures_util::future::join;

fn refused() -> ConfirmAnswer {
    ConfirmAnswer::Ended(ConfirmEnd::Refused)
}

fn read(thread: &str) -> CallRequest {
    crate::support::call("mail.thread.read", &[thread], vec![])
}

#[tokio::test]
async fn two_reads_at_once_raise_one_sheet_and_both_run_on_an_always() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::yielding(vec![always()]);
    let claude = open_wide(&w, CLAUDE).await;
    let (a, b) = join(w.call(&claude, read("t1")), w.call(&claude, read("t2"))).await;
    a.expect("asker runs");
    b.expect("waiter runs on the same yes");
    assert_eq!(w.sheets().len(), 1);
    assert_eq!(grants_of(&w).len(), 1, "the grant is recorded once");
}

#[tokio::test]
async fn a_refusal_refuses_the_waiting_call_too() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::yielding(vec![refused()]);
    let claude = open_wide(&w, CLAUDE).await;
    let (a, b) = join(w.call(&claude, read("t1")), w.call(&claude, read("t2"))).await;
    let want = CallRefusal::Unconfirmed(ConfirmEnd::Refused);
    assert_eq!(a.expect_err("asker"), want);
    assert_eq!(b.expect_err("waiter"), want);
    assert_eq!(w.sheets().len(), 1);
}

#[tokio::test]
async fn an_allow_once_covers_only_the_asker_and_the_waiter_gets_its_own_sheet() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::yielding(vec![once(), refused()]);
    let claude = open_wide(&w, CLAUDE).await;
    let (a, b) = join(w.call(&claude, read("t1")), w.call(&claude, read("t2"))).await;
    a.expect("asker runs once");
    assert_eq!(
        b.expect_err("waiter asks for itself"),
        CallRefusal::Unconfirmed(ConfirmEnd::Refused)
    );
    assert_eq!(w.sheets().len(), 2);
    assert!(grants_of(&w).is_empty());
}

#[tokio::test]
async fn different_actions_each_get_their_own_sheet() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::yielding(vec![once(), once()]);
    let claude = open_wide(&w, CLAUDE).await;
    let (a, b) = join(w.call(&claude, read("t1")), w.call(&claude, files_read())).await;
    a.expect("mail read");
    assert!(!matches!(b, Err(CallRefusal::Unconfirmed(_))), "{b:?}");
    assert_eq!(w.sheets().len(), 2);
}
