//! The launcher's activation token: it reaches the app's `Perform` unchanged when the launcher
//! sent it, and is dropped for every other role, whatever it asked for.

mod support;

use docket_core::{ActivationToken, CallerRole, IntentsReply, IntentsRequest, Invocation};
use docket_router::Router;
use support::*;

fn token() -> ActivationToken {
    ActivationToken::parse("xdg-activation-1234").expect("token")
}

fn performed(router: &Router<docket_fake::FakeSeams>) -> Vec<Invocation> {
    router.seams.link.performed.lock().expect("log").clone()
}

fn perform_with(
    who: &docket_core::CallerId,
    call: docket_core::CallRequest,
    session: Option<prov::SessionId>,
) -> (docket_core::CallerId, IntentsRequest) {
    (
        who.clone(),
        IntentsRequest::Perform {
            activation: Some(token()),
            call,
            session,
            parent_window: None,
        },
    )
}

#[tokio::test]
async fn the_launchers_token_reaches_the_app_unchanged() {
    let router = router();
    let (who, request) = perform_with(&launcher(), call("mail.thread.read", &["t2"], vec![]), None);
    let reply = ask(&router, &who, request).await;
    assert!(
        matches!(reply, IntentsReply::Performed(ref end) if end.is_ok()),
        "{reply:?}"
    );
    let seen = performed(&router);
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].activation, Some(token()));
}

#[tokio::test]
async fn a_call_without_a_token_carries_none() {
    let router = router();
    let reply = ask(
        &router,
        &launcher(),
        IntentsRequest::Perform {
            activation: None,
            call: call("mail.thread.read", &["t2"], vec![]),
            session: None,
            parent_window: None,
        },
    )
    .await;
    assert!(matches!(reply, IntentsReply::Performed(_)), "{reply:?}");
    assert_eq!(performed(&router)[0].activation, None);
}

#[tokio::test]
async fn the_cli_and_the_field_have_the_token_dropped() {
    for (name, who) in [
        ("cli", cli()),
        ("field", caller("org.quire.Field", CallerRole::Field)),
    ] {
        let router = router();
        let (who, request) =
            perform_with(&who, from_cli("mail.thread.read", &["t2"], vec![]), None);
        let reply = ask(&router, &who, request).await;
        assert!(
            matches!(reply, IntentsReply::Performed(ref end) if end.is_ok()),
            "{name}: {reply:?}"
        );
        let seen = performed(&router);
        assert_eq!(seen.len(), 1, "{name}");
        assert_eq!(seen[0].activation, None, "{name}: the token is dropped");
    }
}

#[tokio::test]
async fn the_companion_and_mcp_have_the_token_dropped() {
    for (name, who) in [
        ("companion", companion()),
        ("mcp", caller("org.quire.ActionsMcp", CallerRole::Mcp)),
    ] {
        let router = router();
        let opened = ready(&router).await;
        let (who, request) = perform_with(
            &who,
            call("mail.thread.read", &["t2"], vec![]),
            Some(opened.session.clone()),
        );
        let reply = ask(&router, &who, request).await;
        assert!(
            matches!(reply, IntentsReply::Performed(ref end) if end.is_ok()),
            "{name}: {reply:?}"
        );
        let seen = performed(&router);
        assert_eq!(seen.len(), 1, "{name}");
        assert_eq!(seen[0].activation, None, "{name}: the token is dropped");
    }
}

#[tokio::test]
async fn the_token_is_never_shown_by_debug() {
    let text = format!("{:?}", token());
    assert!(!text.contains("1234"), "{text}");
}

#[test]
fn the_wire_form_is_one_optional_top_level_string() {
    let json = |a: Option<ActivationToken>| {
        let inv = Invocation {
            call: docket_core::CallId(1),
            action: prov::ActionName::parse("mail.thread.read").expect("action"),
            target: docket_core::TargetValue::Nothing,
            args: docket_core::Args::new(),
            actor: prov::Actor::User {
                via: app("org.quire.Shell"),
            },
            origin: docket_core::Origin::Launcher,
            space: prov::SpaceId::desktop(),
            activation: a,
        };
        serde_json::to_value(inv).expect("json")
    };
    assert_eq!(json(Some(token()))["activation"], "xdg-activation-1234");
    assert!(json(None).get("activation").is_none(), "absent when None");
    let back: Invocation = serde_json::from_value(json(None)).expect("an old body parses");
    assert_eq!(back.activation, None);
}
