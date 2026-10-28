//! Wire shapes and words of the wire crate.

use crate::support::*;
use cua_action::{CuaAction, RunMode, WindowSpace, WindowTrust};
use docket_gate_wire::{CuaAsk, EffectBasis};
use docket_permissions_core::Level;
use docket_vocab::{ActionRef, Args, CallRequest, CallerRole, Origin, TargetValue};
use docket_wire::{GrantAsk, IntentsReply, IntentsRequest, Member};
use prov::Effect;
use serde::{Serialize, de::DeserializeOwned};

fn round<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: &T) -> String {
    let json = serde_json::to_string(value).expect("json");
    let back: T = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{json}: {e}"));
    assert_eq!(&back, value, "{json}");
    json
}

#[test]
fn the_cua_ask_carries_an_untrusted_screen_label() {
    let ask = CuaAsk {
        run: prov::RunId::parse("r-1").expect("r"),
        step: 4,
        app: app("org.mozilla.Firefox"),
        trust: WindowTrust::Flatpak,
        mode: RunMode::InPlace,
        space: space("work"),
        action: CuaAction::<WindowSpace>::Observe,
        node: None,
        effect: Effect::Read,
        basis: EffectBasis::DefaultTable,
        screen: untrusted_mail("work"),
    };
    round(&ask);
}

#[test]
fn the_cli_role_word_is_pinned() {
    assert_eq!(round(&CallerRole::Cli), r#""cli""#);
}

#[test]
fn an_old_grant_request_reads_as_always_allow() {
    // A grant request written before limits has no level: it is an always-allow.
    let now = GrantAsk {
        app: app("org.quire.Mail"),
        space: space("work"),
        level: Level::Never,
        run: None,
    };
    let mut old = serde_json::to_value(&now).expect("json");
    old.as_object_mut().expect("object").remove("level");
    let ask: GrantAsk = serde_json::from_value(old).expect("an old request still reads");
    assert_eq!(ask.level, Level::AlwaysAllow);
}

#[test]
fn a_dry_run_is_a_member_of_its_own() {
    let request = IntentsRequest::DryRun {
        call: CallRequest {
            action: ActionRef {
                app: app("org.quire.Mail"),
                name: action("mail.thread.archive"),
            },
            target: TargetValue::Nothing,
            args: Args::new(),
            origin: Origin::Cli,
        },
        session: None,
    };
    assert_eq!(request.member(), Member::DryRun);
    round(&request);
    assert_eq!(
        IntentsRequest::ControlTerminalGrants.member(),
        Member::ControlTerminalGrants
    );
    round(&IntentsReply::TerminalGrants(vec![ActionRef {
        app: app("org.quire.Mail"),
        name: action("mail.thread.archive"),
    }]));
}
