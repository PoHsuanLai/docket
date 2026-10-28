//! The one caller type: how a turn's caller is built from a connection and the stored form it
//! keeps from the old `TurnSource`.

use docket_vocab::{Caller, CallerRole};
use porter_core::AppName;

fn app(name: &str) -> AppName {
    AppName::parse(name).expect("app")
}

#[test]
fn a_turn_is_recorded_as_from_the_surface_that_took_it() {
    let mail = app("org.quire.Mail");
    let rows = [
        (CallerRole::Launcher, Caller::launcher()),
        (CallerRole::Cli, Caller::terminal()),
        (CallerRole::Field, Caller::field(mail.clone())),
        (CallerRole::Editor, Caller::editor(mail.clone())),
        (CallerRole::AcpAgent, Caller::agent(mail.clone())),
        (CallerRole::App, Caller::launcher()),
    ];
    for (role, expected) in rows {
        assert_eq!(Caller::of(role, &mail), expected, "{role:?}");
    }
}

#[test]
fn a_caller_keeps_its_role_and_app() {
    let mail = app("org.quire.Mail");
    let field = Caller::field(mail.clone());
    assert_eq!(field.role(), CallerRole::Field);
    assert_eq!(field.app(), Some(&mail));
    assert_eq!(Caller::terminal().role(), CallerRole::Cli);
    assert_eq!(Caller::terminal().app(), None);
}

#[test]
fn the_stored_form_is_the_old_turn_source() {
    let rows = [
        (Caller::launcher(), r#"{"kind":"launcher"}"#),
        (Caller::terminal(), r#"{"kind":"terminal"}"#),
        (
            Caller::field(app("org.quire.Mail")),
            r#"{"kind":"field","v":"org.quire.Mail"}"#,
        ),
        (
            Caller::editor(app("org.quire.Acp")),
            r#"{"kind":"editor","v":"org.quire.Acp"}"#,
        ),
        (
            Caller::agent(app("org.quire.Agent")),
            r#"{"kind":"agent","v":"org.quire.Agent"}"#,
        ),
    ];
    for (caller, json) in rows {
        assert_eq!(serde_json::to_string(&caller).expect("encode"), json);
        assert_eq!(
            serde_json::from_str::<Caller>(json).expect("decode"),
            caller
        );
    }
}

#[test]
fn a_role_is_its_snake_case_name() {
    assert_eq!(
        serde_json::to_string(&CallerRole::AcpAgent).expect("encode"),
        r#""acp_agent""#
    );
    assert_eq!(CallerRole::ALL.len(), 13);
}
