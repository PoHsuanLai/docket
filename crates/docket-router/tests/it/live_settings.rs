//! The person's settings reach a router that is already serving: the next call reads them.

use crate::support::*;
use docket_core::*;

#[tokio::test]
async fn a_settings_change_applies_to_the_next_call_without_a_restart() {
    let router = router();
    ready(&router).await;
    assert_eq!(router.agent_config(), AgentConfig::default());

    // The person moves Permissions to Manual while the router is serving: the next change asks,
    // where under Auto an archive inside the person's request ran.
    router.apply_settings(AgentConfig {
        permissions: Permissions::Manual,
        ..AgentConfig::default()
    });
    let archive = || call("mail.thread.archive", &["t1"], vec![]);
    let refused = perform(&router, archive()).await.expect_err("asked");
    assert!(
        matches!(refused, CallRefusal::Unconfirmed(_)),
        "{refused:?}"
    );
    assert_eq!(router.seams.confirmer.requests().len(), 1);
    assert!(!router.seams.link.mail.is_archived("t1"));

    // And back: the router follows the latest settings, so the next one runs without a question.
    router.apply_settings(AgentConfig::default());
    perform(&router, archive()).await.expect("ran");
    assert_eq!(
        router.seams.confirmer.requests().len(),
        1,
        "no one else was asked"
    );
    assert!(router.seams.link.mail.is_archived("t1"));
}
