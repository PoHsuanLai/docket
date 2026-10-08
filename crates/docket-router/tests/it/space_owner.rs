//! An app's own Space is that app's alone: a call over it from any other connection is refused
//! whatever the grants and the task policy say, and its owner's own calls go on as before.

use crate::support::*;
use docket_core::*;
use prov::AgentRef;

async fn ready_in(router: &docket_router::Router<docket_fake::FakeSeams>, in_space: &str) {
    let opened = open(router, in_space, AgentRef::Companion).await;
    say(router, &opened.session, "tidy my inbox").await;
    give_policy(router, &opened.session, wide_policy(&opened.task, in_space));
    grant_mail(router, in_space);
}

#[tokio::test]
async fn a_call_over_another_apps_own_space_is_refused_even_with_every_grant() {
    let router = router();
    ready_in(&router, "app:org.quire.Mail:1").await;
    let outcome = perform(&router, call("mail.thread.read", &["t1"], vec![])).await;
    assert_eq!(outcome, Err(CallRefusal::Denied(DenyCode::NotAllowed)));
    assert!(
        router.seams.confirmer.requests().is_empty(),
        "no one is asked"
    );
}

#[tokio::test]
async fn the_owner_shared_and_outside_spaces_go_on_as_before() {
    for in_space in ["app:org.quire.Companiond:1", "work", "desktop"] {
        let router = router();
        ready_in(&router, in_space).await;
        let outcome = perform(&router, call("mail.thread.read", &["t1"], vec![])).await;
        assert_ne!(
            outcome,
            Err(CallRefusal::Denied(DenyCode::NotAllowed)),
            "{in_space}: {outcome:?}"
        );
    }
}
