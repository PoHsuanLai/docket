//! The never-grantable rule and the offer: which scope a standing grant would name, and why not.

use crate::standing::{act, addr, command, files, outbound, path, paths, sends, terminal};
use bulkhead::Cover;
use docket_manifest::{AgentReach, ArgSink, UndoSupport};
use docket_permissions_core::*;
use docket_vocab::ActionRef;
use proptest::prelude::*;
use prov::Effect;

fn facts<'a>(effect: Effect, why: &'a [AskReason], untrusted: &'a [ArgSink]) -> AskFacts<'a> {
    AskFacts {
        effect,
        undo: UndoSupport::Token,
        reach: AgentReach::Offered,
        why,
        untrusted,
        breaker_tripped: false,
        over_budget: false,
        exec: None,
    }
}

fn editor() -> GrantCaller {
    GrantCaller::Editor(prov::ClientName::parse("zed").expect("client"))
}

#[test]
fn the_offer_rule_table() {
    use AskReason as R;
    let rule = [R::Rule(PolicyId("x".into()))];
    type Case<'a> = (&'static str, AskFacts<'a>, Option<Withheld>);
    let plain: &'static [AskReason] = &[R::FirstUse];
    let table: Vec<Case<'_>> = vec![
        (
            "first use of a write",
            facts(Effect::UndoableWrite, plain, &[]),
            None,
        ),
        (
            "outbound effect ask",
            facts(Effect::Outbound, &[R::Effect(Effect::Outbound)], &[]),
            None,
        ),
        (
            "permanent delete",
            facts(Effect::Destructive, plain, &[]),
            Some(Withheld::NeverGrantable(Effect::Destructive)),
        ),
        (
            "outside the task",
            facts(Effect::UndoableWrite, &[R::OutsideTask], &[]),
            Some(Withheld::OutsideTask),
        ),
        (
            "untrusted sink on an outbound",
            facts(
                Effect::Outbound,
                &[R::UntrustedSink(ArgSink::Recipient)],
                &[],
            ),
            Some(Withheld::UntrustedIntoSink),
        ),
        (
            "untrusted body into an outbound by label",
            facts(Effect::Outbound, plain, &[ArgSink::Body]),
            Some(Withheld::UntrustedIntoSink),
        ),
        (
            "untrusted recipient into an undoable write",
            facts(Effect::UndoableWrite, plain, &[ArgSink::Recipient]),
            Some(Withheld::UntrustedIntoSink),
        ),
        (
            "untrusted path into an undoable write",
            facts(Effect::UndoableWrite, plain, &[ArgSink::Path]),
            None,
        ),
        (
            "tainted undoable write",
            facts(Effect::UndoableWrite, &[R::Tainted], &[]),
            None,
        ),
        (
            "tainted outbound",
            facts(Effect::Outbound, &[R::Tainted], &[]),
            Some(Withheld::UntrustedIntoSink),
        ),
        (
            "rule of two",
            facts(Effect::UndoableWrite, &[R::RuleOfTwo], &[]),
            Some(Withheld::UntrustedIntoSink),
        ),
        (
            "action asks every time",
            facts(Effect::UndoableWrite, &[R::AskAlways], &[]),
            Some(Withheld::AsksEveryTime),
        ),
        (
            "cross space",
            facts(Effect::UndoableWrite, &[R::CrossSpace], &[]),
            Some(Withheld::StillAsks(R::CrossSpace)),
        ),
        (
            "evaluation failed",
            facts(Effect::UndoableWrite, &[R::EvaluationFailed], &[]),
            Some(Withheld::StillAsks(R::EvaluationFailed)),
        ),
        (
            "named rule",
            facts(Effect::UndoableWrite, &rule, &[]),
            Some(Withheld::StillAsks(R::Rule(PolicyId("x".into())))),
        ),
    ];
    for (name, f, expect) in table {
        assert_eq!(blocker(&f), expect, "{name}");
    }
}

#[test]
fn a_tripped_breaker_or_spent_budget_withholds_before_anything_else() {
    let mut f = facts(Effect::Destructive, &[AskReason::OutsideTask], &[]);
    f.breaker_tripped = true;
    assert_eq!(blocker(&f), Some(Withheld::BreakerTripped));
    f.breaker_tripped = false;
    f.over_budget = true;
    assert_eq!(blocker(&f), Some(Withheld::OverBudget));
}

#[test]
fn an_ask_every_time_declaration_is_never_grantable() {
    let mut f = facts(Effect::UndoableWrite, &[AskReason::FirstUse], &[]);
    f.reach = AgentReach::AskAlways;
    assert_eq!(blocker(&f), Some(Withheld::AsksEveryTime));
}

#[test]
fn the_offer_names_the_narrowest_scope_or_says_why_not() {
    let f = facts(Effect::UndoableWrite, &[AskReason::FirstUse], &[]);
    let call = paths(&["/home/u/docs/x.txt"]);
    assert_eq!(
        may_offer(&editor(), &call, &f),
        AlwaysOffer::Offered(files("/home/u/docs"))
    );
    let two = paths(&["/home/u/docs/a/x", "/home/u/docs/b/y"]);
    assert_eq!(
        may_offer(&editor(), &two, &f),
        AlwaysOffer::Offered(files("/home/u/docs"))
    );
    let root = paths(&["/x"]);
    assert_eq!(
        may_offer(&editor(), &root, &f),
        AlwaysOffer::Withheld(Withheld::TooBroad)
    );
    let cmd = command("cargo test -p core", "/w/p");
    assert_eq!(
        may_offer(&editor(), &cmd, &f),
        AlwaysOffer::Offered(terminal("cargo test", "/w/p"))
    );
    let flag = command("ls -la", "/w/p");
    assert_eq!(
        may_offer(&editor(), &flag, &f),
        AlwaysOffer::Offered(terminal("ls", "/w/p"))
    );
    let to = sends(vec![addr("a@x.org")]);
    assert_eq!(
        may_offer(&editor(), &to, &f),
        AlwaysOffer::Offered(outbound(addr("a@x.org")))
    );
    let unscoped = CallFacts {
        action: act("mail.thread.archive"),
        args: ArgFacts::Unscoped,
    };
    assert_eq!(
        may_offer(&editor(), &unscoped, &f),
        AlwaysOffer::Withheld(Withheld::Unscoped)
    );
    assert_eq!(
        may_offer(&GrantCaller::Companion, &call, &f),
        AlwaysOffer::Withheld(Withheld::CallerCannotHold)
    );
}

fn any_reason() -> impl Strategy<Value = AskReason> {
    prop_oneof![
        Just(AskReason::FirstUse),
        Just(AskReason::Tainted),
        Just(AskReason::CrossSpace),
        Just(AskReason::AskAlways),
        Just(AskReason::OutsideTask),
        Just(AskReason::RuleOfTwo),
        Just(AskReason::LastingFromUntrusted),
        Just(AskReason::Disagreement),
        Just(AskReason::EvaluationFailed),
        Just(AskReason::Effect(Effect::Outbound)),
        Just(AskReason::UntrustedSink(ArgSink::Path)),
        Just(AskReason::UntrustedSink(ArgSink::Recipient)),
    ]
}

fn any_effect() -> impl Strategy<Value = Effect> {
    prop_oneof![
        Just(Effect::Read),
        Just(Effect::UndoableWrite),
        Just(Effect::Outbound),
        Just(Effect::Destructive),
    ]
}

proptest! {
    // Whatever the ask, a grant can stand in for it only when none of R1's never-grantable
    // conditions holds; the offer is made under the same condition.
    #[test]
    fn nothing_never_grantable_is_ever_offered_or_lifted(
        effect in any_effect(),
        why in proptest::collection::vec(any_reason(), 0..4),
        untrusted in proptest::collection::vec(
            prop_oneof![Just(ArgSink::Body), Just(ArgSink::Path), Just(ArgSink::Recipient)], 0..3),
        tripped in any::<bool>(),
        spent in any::<bool>(),
        ask_always in any::<bool>(),
        irreversible in any::<bool>(),
    ) {
        let f = AskFacts {
            effect,
            undo: if irreversible { UndoSupport::NotUndoable } else { UndoSupport::Token },
            reach: if ask_always { AgentReach::AskAlways } else { AgentReach::Offered },
            why: &why,
            untrusted: &untrusted,
            breaker_tripped: tripped,
            over_budget: spent,
            exec: None,
        };
        let offered = matches!(
            may_offer(&editor(), &paths(&["/a/b/c"]), &f),
            AlwaysOffer::Offered(_)
        );
        let lifts = blocker(&f).is_none();
        if offered { prop_assert!(lifts); }
        if lifts {
            prop_assert!(!tripped && !spent && !ask_always);
            prop_assert!(effect != Effect::Destructive);
            prop_assert!(!why.contains(&AskReason::OutsideTask));
            prop_assert!(!why.contains(&AskReason::RuleOfTwo));
            let risky = effect >= Effect::Outbound || irreversible;
            if risky { prop_assert!(untrusted.is_empty()); }
        }
    }

    #[test]
    fn a_path_scope_never_covers_a_sibling_prefix(
        base in "(/[a-z]{1,4}){1,3}",
        tail in "[a-z]{1,3}",
    ) {
        let under = path(&base);
        let sibling = path(&format!("{base}{tail}"));
        prop_assert_eq!(under.covers(&sibling), Cover::Misses);
        let child = path(&format!("{base}/{tail}"));
        prop_assert_eq!(under.covers(&child), Cover::Covers);
    }
}

// R11: the execute taint, narrowed.

#[test]
fn an_executes_session_taint_excuses_only_a_command_of_its_own_with_no_way_out() {
    use AskReason as R;
    let taint = [R::Tainted, R::RuleOfTwo, R::UntrustedSink(ArgSink::Body)];
    let programs = NetPrograms {
        any: vec!["curl".to_owned()],
        ..NetPrograms::default()
    };
    let exec =
        |derives, network, line| Some(ExecFacts::from_choices(derives, network, line, &programs));
    let with = |exec| AskFacts {
        exec,
        ..facts(Effect::Outbound, &taint, &[ArgSink::Body])
    };
    let own = with(exec(Some("own"), Some("closed"), "cargo test"));
    assert_eq!(blocker(&own), None);
    let derived = with(exec(Some("read"), Some("closed"), "cargo test"));
    assert_eq!(blocker(&derived), Some(Withheld::UntrustedIntoSink));
    let out = with(exec(Some("own"), Some("closed"), "curl a.test"));
    assert_eq!(blocker(&out), Some(Withheld::CanSendOut));
    let open = with(exec(Some("own"), Some("open"), "cargo test"));
    assert_eq!(blocker(&open), Some(Withheld::CanSendOut));
    // Every other call keeps the rule.
    assert_eq!(blocker(&with(None)), Some(Withheld::UntrustedIntoSink));
    // A reason that is not the session's taint is still a reason.
    let outside = AskFacts {
        exec: exec(Some("own"), Some("closed"), "ls"),
        ..facts(Effect::Outbound, &[R::OutsideTask], &[])
    };
    assert_eq!(blocker(&outside), Some(Withheld::OutsideTask));
    // A tripped breaker comes first, and a way out beats the other reasons.
    let tripped = AskFacts {
        breaker_tripped: true,
        ..out
    };
    assert_eq!(blocker(&tripped), Some(Withheld::BreakerTripped));
}

fn agent() -> GrantCaller {
    GrantCaller::AcpAgent(ProgramName::parse("claude-code").expect("program"))
}

fn mail_read(name: &str) -> CallFacts {
    CallFacts {
        action: ActionRef {
            app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
            name: prov::ActionName::parse(name).expect("action"),
        },
        args: ArgFacts::Unscoped,
    }
}

#[test]
fn an_agents_read_is_offered_the_one_action_even_when_the_session_is_tainted() {
    let call = mail_read("mail.thread.search");
    let f = facts(
        Effect::Read,
        &[AskReason::FirstUse, AskReason::Tainted],
        &[],
    );
    assert_eq!(
        may_offer(&agent(), &call, &f),
        AlwaysOffer::Offered(StandingScope::Reads {
            action: call.action.clone()
        })
    );
    // An editor, a write, a hidden or ask-always action and the agent pseudo-app are not offered it.
    assert_ne!(
        may_offer(&editor(), &call, &f),
        AlwaysOffer::Offered(StandingScope::Reads {
            action: call.action.clone()
        })
    );
    for effect in [Effect::UndoableWrite, Effect::Outbound, Effect::Destructive] {
        let w = facts(effect, &[AskReason::FirstUse], &[]);
        assert!(
            !matches!(
                may_offer(&agent(), &call, &w),
                AlwaysOffer::Offered(StandingScope::Reads { .. })
            ),
            "{effect:?}"
        );
    }
    let mut ask = f;
    ask.reach = AgentReach::AskAlways;
    assert_eq!(
        may_offer(&agent(), &call, &ask),
        AlwaysOffer::Withheld(Withheld::AsksEveryTime)
    );
    let pseudo = CallFacts {
        action: acp_agent_action(FILES_READ).expect("action"),
        args: ArgFacts::Unscoped,
    };
    assert!(!matches!(
        may_offer(&agent(), &pseudo, &f),
        AlwaysOffer::Offered(StandingScope::Reads { .. })
    ));
}

#[test]
fn a_read_scope_covers_its_action_only_and_never_the_pseudo_app() {
    let search = mail_read("mail.thread.search");
    let scope = StandingScope::Reads {
        action: search.action.clone(),
    };
    assert_eq!(scope.covers(&search), Cover::Covers);
    for other in [
        "mail.message.forward",
        "mail.message.send",
        "mail.thread.read",
    ] {
        assert_eq!(scope.covers(&mail_read(other)), Cover::Misses, "{other}");
    }
    let pseudo = CallFacts {
        action: acp_agent_action(FILES_READ).expect("action"),
        args: ArgFacts::Unscoped,
    };
    let on_pseudo = StandingScope::Reads {
        action: pseudo.action.clone(),
    };
    assert_eq!(on_pseudo.covers(&pseudo), Cover::Misses);
    let grant = StandingGrant::new(agent(), scope, prov::UnixSeconds(1));
    let other = GrantCaller::AcpAgent(ProgramName::parse("gemini-cli").expect("program"));
    assert_eq!(grant.covers(&other, &search), Cover::Misses);
}

#[test]
fn a_damaged_standing_text_is_a_typed_fault_not_an_empty_list() {
    for text in ["{", "[1]", "{\"not\": \"a list\"}"] {
        assert!(
            matches!(decode_standing(text), Err(StandingFileFault::Decode(_))),
            "{text:?}"
        );
    }
}
