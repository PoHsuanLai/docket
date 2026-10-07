//! The named hard rules of the policy set (Rule of Two, untrusted sink) and the reasons a
//! cell gives when it asks.

use crate::support::*;
use docket_core::*;
use prov::{Effect, Integrity};

#[test]
fn rule_of_two_requires_confirm_in_every_strictness() {
    let seen = SessionSaw {
        private: Saw::Seen,
        untrusted: Saw::Seen,
    };
    for s in [
        Strictness::AskMore,
        Strictness::Default,
        Strictness::TrustMore,
    ] {
        let got = pdp().decide(&with(Effect::Outbound, |r| {
            strictness(s)(r);
            r.context.saw = seen;
        }));
        assert!(
            matches!(&got, Ruling::Ask(why) if why.contains(&AskReason::RuleOfTwo)),
            "{s:?}: {got:?}"
        );
    }
}

#[test]
fn rule_of_two_off_when_only_two_legs() {
    let rows = [
        ("no private", Saw::NotSeen, Saw::Seen),
        ("no untrusted", Saw::Seen, Saw::NotSeen),
        ("neither", Saw::NotSeen, Saw::NotSeen),
    ];
    for (name, private, untrusted) in rows {
        // Outbound is judged only under TrustMore (QUESTIONS S1); under Default it asks for
        // the effect alone, never for the Rule of Two.
        let got = pdp().decide(&with(Effect::Outbound, |r| {
            strictness(Strictness::TrustMore)(r);
            r.context.saw = SessionSaw { private, untrusted };
        }));
        assert_eq!(cell(&got), Cell::Judged, "{name}: {got:?}");
        let got = pdp().decide(&with(Effect::Outbound, |r| {
            r.context.saw = SessionSaw { private, untrusted };
        }));
        assert_eq!(
            asks(&got),
            [AskReason::Effect(Effect::Outbound)],
            "{name}: {got:?}"
        );
    }
}

#[test]
fn rule_of_two_does_not_apply_to_other_effects() {
    let seen = SessionSaw {
        private: Saw::Seen,
        untrusted: Saw::Seen,
    };
    let got = pdp().decide(&with(Effect::UndoableWrite, |r| r.context.saw = seen));
    assert_eq!(cell(&got), Cell::Final, "{got:?}");
}

#[test]
fn untrusted_recipient_asks_even_inside_task_policy() {
    for effect in [Effect::Outbound, Effect::Destructive] {
        let got = pdp().decide(&with(effect, |r| {
            strictness(Strictness::TrustMore)(r);
            untrust_recipient(r);
        }));
        assert!(
            asks(&got).contains(&AskReason::UntrustedSink(ArgSink::Recipient)),
            "{effect:?}: {got:?}"
        );
    }
}

#[test]
fn every_sink_has_its_own_reason() {
    type Spoil = fn(&mut SinkIntegrity);
    let rows: [(&str, Spoil, ArgSink); 4] = [
        (
            "recipient",
            |s| s.recipient = Integrity::Untrusted,
            ArgSink::Recipient,
        ),
        (
            "destination",
            |s| s.destination = Integrity::Untrusted,
            ArgSink::Destination,
        ),
        ("path", |s| s.path = Integrity::Untrusted, ArgSink::Path),
        ("body", |s| s.body = Integrity::Untrusted, ArgSink::Body),
    ];
    for (name, edit, sink) in rows {
        let got = pdp().decide(&with(Effect::Outbound, |r| edit(&mut r.context.sinks)));
        assert_eq!(
            asks(&got),
            [AskReason::UntrustedSink(sink)],
            "{name}: {got:?}"
        );
    }
}

#[test]
fn an_untrusted_sink_does_not_ask_an_undoable_write() {
    let got = pdp().decide(&with(Effect::UndoableWrite, |r| {
        r.context.sinks.body = Integrity::Untrusted;
    }));
    assert_eq!(cell(&got), Cell::Judged, "{got:?}");
}

#[test]
fn the_forbid_rules_are_named_in_the_reasons() {
    let got = pdp().decide(&with(Effect::Outbound, |r| {
        r.context.saw = SessionSaw {
            private: Saw::Seen,
            untrusted: Saw::Seen,
        };
        untrust_recipient(r);
    }));
    let why = asks(&got);
    assert!(why.contains(&AskReason::RuleOfTwo), "{why:?}");
    assert!(
        why.contains(&AskReason::UntrustedSink(ArgSink::Recipient)),
        "{why:?}"
    );
}
