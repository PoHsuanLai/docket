//! The terminal rows of the decision grid (cli.md section 3): `Actor::Cli`, the same in every
//! strictness. A read is final; an undoable, outbound or destructive act asks and never goes to
//! a reviewer; an action that asks every time asks; a hidden action is denied. A standing grant
//! the person gave from the sheet is the only way to skip the ask, and it lifts nothing else.

use docket_core::*;
use policy_point::{GrantState, PolicyRequest, TerminalGrant};
use prov::{ActorKind, Effect, Integrity};
mod support;
use support::*;

const EVERY_STRICTNESS: [Strictness; 3] = [
    Strictness::AskMore,
    Strictness::Default,
    Strictness::TrustMore,
];

/// A call from `quire-do`: arguments are untrusted (the person or an agent typed them), no
/// class grants exist for a terminal, and the call is outside any derived task policy.
fn cli(effect: Effect, edit: impl FnOnce(&mut PolicyRequest)) -> PolicyRequest {
    with(effect, |r| {
        r.principal.kind = ActorKind::Cli;
        r.context.origin = Origin::Cli;
        r.context.args = Integrity::Untrusted;
        r.context.planner = Integrity::Untrusted;
        r.context.coverage = CoverageState::Outside;
        r.context.grant = GrantState::Once;
        r.context.terminal = TerminalGrant::NotGranted;
        edit(r);
    })
}

fn granted(r: &mut PolicyRequest) {
    r.context.terminal = TerminalGrant::Granted;
}

use policy_point::CoverageState;

#[test]
fn terminal_grid() {
    use Cell::{Ask, Deny, Final};
    use Effect::{Destructive, Outbound, Read, UndoableWrite};
    let pdp = pdp();
    // (row, effect, edit, want)
    type Edit = fn(&mut PolicyRequest);
    let rows: Vec<(&str, Effect, Edit, Cell)> = vec![
        ("read", Read, |_| {}, Final),
        ("undoable asks", UndoableWrite, |_| {}, Ask),
        ("outbound asks", Outbound, |_| {}, Ask),
        ("destructive asks", Destructive, |_| {}, Ask),
        ("ask_always read asks", Read, ask_always, Ask),
        ("ask_always undoable asks", UndoableWrite, ask_always, Ask),
        ("hidden read is denied", Read, hidden, Deny),
        ("hidden undoable is denied", UndoableWrite, hidden, Deny),
        ("hidden outbound is denied", Outbound, hidden, Deny),
        ("hidden destructive is denied", Destructive, hidden, Deny),
        ("denied consent is denied", Read, denied, Deny),
        ("read with no class grant", Read, no_grant, Final),
        (
            "undoable trusted and inside still asks",
            UndoableWrite,
            trusted_inside,
            Ask,
        ),
        (
            "outbound trusted and inside still asks",
            Outbound,
            trusted_inside,
            Ask,
        ),
        (
            "undoable with a standing grant",
            UndoableWrite,
            granted,
            Final,
        ),
        ("outbound with a standing grant", Outbound, granted, Final),
        (
            "destructive with a standing grant asks",
            Destructive,
            granted,
            Ask,
        ),
        (
            "ask_always with a standing grant asks",
            UndoableWrite,
            |r| {
                granted(r);
                ask_always(r);
            },
            Ask,
        ),
        (
            "hidden with a standing grant is denied",
            UndoableWrite,
            |r| {
                granted(r);
                hidden(r);
            },
            Deny,
        ),
        (
            "outbound grant, untrusted recipient asks",
            Outbound,
            |r| {
                granted(r);
                untrust_recipient(r);
            },
            Ask,
        ),
        (
            "outbound grant after the rule of two asks",
            Outbound,
            |r| {
                granted(r);
                r.context.saw = SessionSaw {
                    private: Saw::Seen,
                    untrusted: Saw::Seen,
                };
            },
            Ask,
        ),
        (
            "undoable grant over the mass threshold asks",
            UndoableWrite,
            |r| {
                granted(r);
                r.context.count = porter_core::Count(21);
            },
            Ask,
        ),
        (
            "undoable grant, lasting memory from untrusted asks",
            UndoableWrite,
            |r| {
                granted(r);
                r.resource.lasting = Lasting::AgentMemory;
            },
            Ask,
        ),
        ("read with a standing grant", Read, granted, Final),
    ];
    println!(
        "{:<52} {:>9} {:>9} {:>9}",
        "row", "ask_more", "default", "trust_more"
    );
    for (name, effect, edit, want) in &rows {
        let got: Vec<Cell> = EVERY_STRICTNESS
            .iter()
            .map(|strictness| {
                let request = cli(*effect, |r| {
                    r.context.strictness = *strictness;
                    edit(r);
                });
                let ruling = pdp.decide(&request);
                let cell = cell(&ruling);
                assert_eq!(cell, *want, "{name} ({strictness:?}): {ruling:?}");
                cell
            })
            .collect();
        println!("{name:<52} {:>9?} {:>9?} {:>9?}", got[0], got[1], got[2]);
    }
}

fn ask_always(r: &mut PolicyRequest) {
    r.resource.reach = AgentReach::AskAlways;
}
fn hidden(r: &mut PolicyRequest) {
    r.resource.reach = AgentReach::Hidden;
}
fn denied(r: &mut PolicyRequest) {
    r.context.grant = GrantState::Denied;
}
fn no_grant(r: &mut PolicyRequest) {
    r.context.grant = GrantState::None;
}
fn trusted_inside(r: &mut PolicyRequest) {
    r.context.args = Integrity::Trusted;
    r.context.planner = Integrity::Trusted;
    r.context.coverage = CoverageState::Inside;
}

#[test]
fn a_terminal_never_goes_to_review() {
    // Every non-read row, in every strictness, trusted or not, inside or not: Ask or Final,
    // never `AllowJudged`.
    let pdp = pdp();
    for strictness in EVERY_STRICTNESS {
        for effect in [Effect::UndoableWrite, Effect::Outbound, Effect::Destructive] {
            for terminal in [TerminalGrant::NotGranted, TerminalGrant::Granted] {
                for (inside, trusted) in
                    [(true, true), (true, false), (false, true), (false, false)]
                {
                    let request = cli(effect, |r| {
                        r.context.strictness = strictness;
                        r.context.terminal = terminal;
                        if inside {
                            r.context.coverage = CoverageState::Inside;
                        }
                        if trusted {
                            r.context.args = Integrity::Trusted;
                            r.context.planner = Integrity::Trusted;
                        }
                    });
                    let got = pdp.decide(&request);
                    assert_ne!(
                        cell(&got),
                        Cell::Judged,
                        "{effect:?} {strictness:?} {terminal:?} inside={inside} trusted={trusted}: {got:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_terminal_names_its_reason() {
    let got = pdp().decide(&cli(Effect::UndoableWrite, |_| {}));
    assert_eq!(asks(&got), [AskReason::FromTerminal], "{got:?}");
    let got = pdp().decide(&cli(Effect::Destructive, granted));
    assert_eq!(
        asks(&got),
        [AskReason::Effect(Effect::Destructive)],
        "{got:?}"
    );
    let got = pdp().decide(&cli(Effect::Outbound, |r| {
        untrust_recipient(r);
    }));
    assert!(asks(&got).contains(&AskReason::FromTerminal), "{got:?}");
    assert!(
        asks(&got)
            .iter()
            .any(|r| matches!(r, AskReason::UntrustedSink(_))),
        "{got:?}"
    );
}

#[test]
fn a_standing_grant_is_for_the_terminal_alone() {
    // The same context on any other caller is not a grant: a companion still takes its own row.
    let pdp = pdp();
    for kind in [ActorKind::Companion, ActorKind::Mcp, ActorKind::Cua] {
        let request = with(Effect::Outbound, |r| {
            r.principal.kind = kind;
            r.context.terminal = TerminalGrant::Granted;
            r.context.coverage = CoverageState::Outside;
        });
        assert_ne!(cell(&pdp.decide(&request)), Cell::Final, "{kind:?}");
    }
}
