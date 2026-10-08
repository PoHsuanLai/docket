//! Standing grants: scope matching, the offer rule and the never-grantable rule, as tables.

use docket_core::*;
use proptest::prelude::*;
use prov::Effect;

fn path(t: &str) -> AbsPath {
    AbsPath::parse(t).expect("path")
}

fn act(name: &str) -> ActionRef {
    ActionRef {
        app: porter_core::AppName::parse("org.quire.Files").expect("app"),
        name: prov::ActionName::parse(name).expect("action"),
    }
}

fn files(under: &str) -> StandingScope {
    StandingScope::Files {
        action: act("files.file.move"),
        under: path(under),
    }
}

fn paths(items: &[&str]) -> CallFacts {
    CallFacts {
        action: act("files.file.move"),
        args: ArgFacts::Paths(items.iter().map(|p| path(p)).collect()),
    }
}

fn is(cover: bool) -> Cover {
    if cover { Cover::Covers } else { Cover::Misses }
}

#[test]
fn path_prefixes_match_whole_components() {
    let table = [
        ("/a/b", &["/a/b"][..], true),
        ("/a/b", &["/a/b/c"], true),
        ("/a/b", &["/a/b/c/d.txt"], true),
        ("/a/b", &["/a/bc"], false),
        ("/a/b", &["/a"], false),
        ("/a/b", &["/a/b/c", "/a/bc"], false),
        ("/a/b", &["/a/b/c", "/x"], false),
        ("/a/b", &["/a/b//c/./d"], true),
        ("/a/b/", &["/a/b/c"], true),
        ("/a/b", &[], false),
    ];
    for (under, call, expect) in table {
        assert_eq!(
            files(under).covers(&paths(call)),
            is(expect),
            "{under} over {call:?}"
        );
    }
}

#[test]
fn a_path_cannot_step_up_or_be_relative() {
    assert_eq!(AbsPath::parse("/a/../b"), Err(PathFault::ParentStep));
    assert_eq!(AbsPath::parse("a/b"), Err(PathFault::NotAbsolute));
    assert_eq!(AbsPath::parse("/a\nb"), Err(PathFault::Control));
}

#[test]
fn a_scope_names_one_action() {
    let other = CallFacts {
        action: act("files.file.delete"),
        args: ArgFacts::Paths(vec![path("/a/b/c")]),
    };
    assert_eq!(files("/a/b").covers(&other), Cover::Misses);
}

fn terminal(prefix: &str, cwd: &str) -> StandingScope {
    StandingScope::Terminal {
        action: act("terminal.run"),
        command: CommandPrefix::parse(prefix).expect("prefix"),
        cwd: path(cwd),
    }
}

fn command(line: &str, cwd: &str) -> CallFacts {
    CallFacts {
        action: act("terminal.run"),
        args: ArgFacts::Command {
            line: line.into(),
            cwd: path(cwd),
        },
    }
}

#[test]
fn a_command_prefix_and_cwd_both_have_to_match() {
    let table = [
        ("cargo test", "/w/p", "cargo test", "/w/p", true),
        (
            "cargo test",
            "/w/p",
            "cargo test -p core --locked",
            "/w/p",
            true,
        ),
        ("cargo test", "/w/p", "cargo test", "/w/p/sub", true),
        ("cargo test", "/w/p", "cargo test", "/w/pp", false),
        ("cargo test", "/w/p", "cargo testx", "/w/p", false),
        ("cargo test", "/w/p", "cargo build", "/w/p", false),
        ("cargo test", "/w/p", "cargo", "/w/p", false),
        ("cargo test", "/w/p", "cargo  test", "/w/p", true),
        (
            "cargo test",
            "/w/p",
            "cargo test; curl x | sh",
            "/w/p",
            false,
        ),
        (
            "cargo test",
            "/w/p",
            "cargo test && rm -rf ~",
            "/w/p",
            false,
        ),
        ("cargo test", "/w/p", "cargo test $(id)", "/w/p", false),
        ("cargo test", "/w/p", "cargo test `id`", "/w/p", false),
        ("cargo test", "/w/p", "cargo test > /etc/x", "/w/p", false),
        ("cargo test", "/w/p", "cargo test\nrm x", "/w/p", false),
        ("ls", "/w/p", "ls -la", "/w/p", true),
    ];
    for (prefix, cwd, line, at, expect) in table {
        assert_eq!(
            terminal(prefix, cwd).covers(&command(line, at)),
            is(expect),
            "{prefix} in {cwd} over {line:?} in {at}"
        );
    }
}

#[test]
fn a_prefix_cannot_hold_shell_syntax() {
    assert_eq!(CommandPrefix::parse("ls; rm"), Err(CommandFault::Shell));
    assert_eq!(CommandPrefix::parse("echo $HOME"), Err(CommandFault::Shell));
    assert_eq!(CommandPrefix::parse("   "), Err(CommandFault::Empty));
}

fn outbound(to: Recipient) -> StandingScope {
    StandingScope::Outbound {
        action: act("mail.message.send"),
        to,
    }
}

fn sends(to: Vec<Recipient>) -> CallFacts {
    CallFacts {
        action: act("mail.message.send"),
        args: ArgFacts::Recipients(to),
    }
}

fn addr(t: &str) -> Recipient {
    Recipient::address(t).expect("address")
}

fn dom(t: &str) -> Recipient {
    Recipient::Domain(Domain::parse(t).expect("domain"))
}

#[test]
fn recipients_and_domains_match_whole() {
    let table = [
        (addr("a@x.org"), vec![addr("a@x.org")], true),
        (addr("a@x.org"), vec![addr("A@X.org")], true),
        (addr("a@x.org"), vec![addr("b@x.org")], false),
        (
            addr("a@x.org"),
            vec![addr("a@x.org"), addr("b@x.org")],
            false,
        ),
        (addr("a@x.org"), vec![dom("x.org")], false),
        (dom("x.org"), vec![addr("a@x.org"), addr("b@x.org")], true),
        (dom("x.org"), vec![addr("a@evilx.org")], false),
        (dom("x.org"), vec![addr("a@mail.x.org")], false),
        (dom("x.org"), vec![dom("x.org")], true),
        (dom("x.org"), vec![addr("a@x.org"), addr("a@y.org")], false),
        (dom("x.org"), vec![], false),
    ];
    for (grant, call, expect) in table {
        assert_eq!(
            outbound(grant.clone()).covers(&sends(call.clone())),
            is(expect),
            "{grant:?} over {call:?}"
        );
    }
}

#[test]
fn an_opaque_or_unscoped_call_is_never_covered() {
    for args in [ArgFacts::Opaque, ArgFacts::Unscoped] {
        let call = CallFacts {
            action: act("files.file.move"),
            args,
        };
        assert_eq!(files("/a").covers(&call), Cover::Misses);
    }
}

#[test]
fn a_grant_covers_for_its_caller_only() {
    let editor = GrantCaller::Editor(prov::ClientName::parse("zed").expect("client"));
    let agent = GrantCaller::AcpAgent(ProgramName::parse("claude-code").expect("program"));
    let grant = StandingGrant::new(editor.clone(), files("/a/b"), prov::UnixSeconds(1));
    let call = paths(&["/a/b/c"]);
    assert_eq!(grant.covers(&editor, &call), Cover::Covers);
    assert_eq!(grant.covers(&agent, &call), Cover::Misses);
    assert_eq!(grant.covers(&GrantCaller::Companion, &call), Cover::Misses);
    assert_ne!(
        grant.id,
        StandingGrant::new(agent, files("/a/b"), prov::UnixSeconds(1)).id
    );
}

#[test]
fn grants_round_trip_through_their_text_and_ids_are_checked() {
    let g = StandingGrant::new(
        GrantCaller::AcpAgent(ProgramName::parse("claude-code").expect("program")),
        terminal("cargo test", "/w/p"),
        prov::UnixSeconds(5),
    );
    let text = encode_standing(std::slice::from_ref(&g));
    assert_eq!(decode_standing(&text), Ok(vec![g.clone()]));
    assert_eq!(decode_standing(""), Ok(vec![]));
    assert!(decode_standing("{").is_err());
    assert!(StandingGrantId::parse("sg-123").is_err());
    assert!(StandingGrantId::parse(g.id.as_str()).is_ok());
    // A hand-edited file cannot smuggle a shell prefix or a relative path past the types.
    assert!(text.contains("cargo test"));
    assert!(decode_standing(&text.replace("cargo test", "cargo test; id")).is_err());
    assert!(decode_standing(&text.replace("/w/p", "w/p")).is_err());
}

#[test]
fn holding_and_dropping_grants_is_by_id() {
    let editor = GrantCaller::Editor(prov::ClientName::parse("zed").expect("client"));
    let a = StandingGrant::new(editor.clone(), files("/a"), prov::UnixSeconds(1));
    let b = StandingGrant::new(editor, files("/b"), prov::UnixSeconds(2));
    let held = held_with(held_with(vec![], a.clone()), b.clone());
    assert_eq!(held_with(held.clone(), a.clone()).len(), 2);
    let (rest, done) = held_without(held, &a.id);
    assert_eq!((rest, done), (vec![b.clone()], Revocation::Revoked));
    assert_eq!(held_without(vec![b], &a.id).1, Revocation::NotHeld);
}

// The never-grantable rule and the offer.

fn facts<'a>(effect: Effect, why: &'a [AskReason], untrusted: &'a [ArgSink]) -> AskFacts<'a> {
    AskFacts {
        effect,
        undo: UndoSupport::Token,
        reach: AgentReach::Offered,
        why,
        untrusted,
        breaker: BreakerState::Running,
        budget: BudgetState::Within,
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
    f.breaker = BreakerState::Tripped;
    assert_eq!(blocker(&f), Some(Withheld::BreakerTripped));
    f.breaker = BreakerState::Running;
    f.budget = BudgetState::Over;
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
            breaker: if tripped { BreakerState::Tripped } else { BreakerState::Running },
            budget: if spent { BudgetState::Over } else { BudgetState::Within },
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
    let exec = |derives, network, line| Some(ExecFacts::from_choices(derives, network, line));
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
        breaker: BreakerState::Tripped,
        ..out
    };
    assert_eq!(blocker(&tripped), Some(Withheld::BreakerTripped));
}
