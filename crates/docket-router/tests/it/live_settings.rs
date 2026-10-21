//! The person's settings reach a router that is already serving: the next call reads them.

use crate::support::*;
use docket_core::*;

#[tokio::test]
async fn a_settings_change_applies_to_the_next_call_without_a_restart() {
    let router = router();
    ready(&router).await;
    assert_eq!(router.agent_config(), AgentConfig::default());

    // The person sets Auto's "inside" row to Asks while the router is serving: the next change
    // asks, where an archive inside the person's request ran.
    let mut asking = AgentConfig::default();
    asking.rows.auto.inside = InsideRow::Asks;
    router.apply_settings(asking);
    let archive = |thread: &str| call("mail.thread.archive", &[thread], vec![]);
    let refused = perform(&router, archive("t1")).await.expect_err("asked");
    assert!(
        matches!(refused, CallRefusal::Unconfirmed(_)),
        "{refused:?}"
    );
    assert_eq!(router.seams.confirmer.requests().len(), 1);
    assert!(!router.seams.link.mail.is_archived("t1"));

    // And back: the router follows the latest settings, so the next one runs without a question.
    // (Another thread: the same call again would be a repeat of the one refused.)
    router.apply_settings(AgentConfig::default());
    perform(&router, archive("t2")).await.expect("ran");
    assert_eq!(
        router.seams.confirmer.requests().len(),
        1,
        "no one else was asked"
    );
    assert!(router.seams.link.mail.is_archived("t2"));
}
