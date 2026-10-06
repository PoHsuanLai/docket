//! The person's settings reach a router that is already serving: the next call reads them.

mod support;

use docket_core::*;
use docket_router::Router;
use support::*;

fn contact() -> Value {
    Value::Entity(entity("mail.contact", "c1"))
}

/// A send whose words the person's request ("tidy my inbox") covers; two sends differ in them, so
/// the second is not a repeat of the first (a repeat is its own refusal).
fn send(words: &str) -> CallRequest {
    call(
        "mail.message.send",
        &[],
        vec![("to", contact()), ("body", Value::Text(words.into()))],
    )
}

async fn show_contacts(router: &Router<docket_fake::FakeSeams>) {
    let reply = ask(
        router,
        &companion(),
        IntentsRequest::Suggest(SuggestAsk {
            action: action("mail.message.send"),
            param: param("to"),
            typed: String::new(),
        }),
    )
    .await;
    assert!(
        matches!(reply, IntentsReply::Suggestions(ref s) if s.len() == 1),
        "{reply:?}"
    );
}

#[tokio::test]
async fn a_settings_change_applies_to_the_next_call_without_a_restart() {
    let router = router();
    ready(&router).await;
    show_contacts(&router).await;
    assert_eq!(router.agent_config(), AgentConfig::default());

    // The person moves strictness to Trust more while the router is serving: the next outbound
    // act, to a recipient the person's own contacts vouch for, runs on the reviewers' word.
    router.apply_settings(AgentConfig {
        strictness: Strictness::TrustMore,
        ..AgentConfig::default()
    });
    perform(&router, send("tidy")).await.expect("ran");
    assert!(
        router.seams.confirmer.requests().is_empty(),
        "no one was asked"
    );
    assert_eq!(router.seams.link.mail.sent().len(), 1);

    // And back: the router follows the latest settings, so the next one asks the person (who
    // dismisses the sheet here).
    router.apply_settings(AgentConfig::default());
    let refused = perform(&router, send("tidy my inbox"))
        .await
        .expect_err("asked");
    assert!(
        matches!(refused, CallRefusal::Unconfirmed(_)),
        "{refused:?}"
    );
    assert_eq!(router.seams.confirmer.requests().len(), 1);
    assert_eq!(
        router.seams.link.mail.sent().len(),
        1,
        "nothing more was sent"
    );
}
