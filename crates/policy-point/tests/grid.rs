//! The decision grid of actions-addendum A2, one row per cell and strictness.

use docket_core::*;
use prov::Effect;
mod support;
use support::*;

#[test]
fn default_policy_grid() {
    use Cell::{Ask, Final, Judged};
    use Effect::{Destructive, Outbound, Read, UndoableWrite};
    use Strictness::{AskMore, Default, TrustMore};
    let pdp = pdp();
    // (name, effect, strictness, trusted?, inside?, want)
    let rows: [(&str, Effect, Strictness, bool, bool, Cell); 36] = [
        ("read ask_more", Read, AskMore, true, true, Final),
        ("read default", Read, Default, true, true, Final),
        ("read trust_more", Read, TrustMore, true, true, Final),
        (
            "read untrusted outside ask_more",
            Read,
            AskMore,
            false,
            false,
            Final,
        ),
        (
            "read untrusted outside default",
            Read,
            Default,
            false,
            false,
            Final,
        ),
        (
            "read untrusted outside trust_more",
            Read,
            TrustMore,
            false,
            false,
            Final,
        ),
        (
            "undoable T In ask_more",
            UndoableWrite,
            AskMore,
            true,
            true,
            Judged,
        ),
        (
            "undoable T In default",
            UndoableWrite,
            Default,
            true,
            true,
            Final,
        ),
        (
            "undoable T In trust_more",
            UndoableWrite,
            TrustMore,
            true,
            true,
            Final,
        ),
        (
            "undoable T Out ask_more",
            UndoableWrite,
            AskMore,
            true,
            false,
            Ask,
        ),
        (
            "undoable T Out default",
            UndoableWrite,
            Default,
            true,
            false,
            Judged,
        ),
        (
            "undoable T Out trust_more",
            UndoableWrite,
            TrustMore,
            true,
            false,
            Judged,
        ),
        (
            "undoable U In ask_more",
            UndoableWrite,
            AskMore,
            false,
            true,
            Ask,
        ),
        (
            "undoable U In default",
            UndoableWrite,
            Default,
            false,
            true,
            Judged,
        ),
        (
            "undoable U In trust_more",
            UndoableWrite,
            TrustMore,
            false,
            true,
            Judged,
        ),
        (
            "undoable U Out ask_more",
            UndoableWrite,
            AskMore,
            false,
            false,
            Ask,
        ),
        ("outbound T In ask_more", Outbound, AskMore, true, true, Ask),
        ("outbound T In default", Outbound, Default, true, true, Ask),
        (
            "outbound T In trust_more",
            Outbound,
            TrustMore,
            true,
            true,
            Judged,
        ),
        (
            "outbound T Out ask_more",
            Outbound,
            AskMore,
            true,
            false,
            Ask,
        ),
        (
            "outbound T Out default",
            Outbound,
            Default,
            true,
            false,
            Ask,
        ),
        (
            "outbound T Out trust_more",
            Outbound,
            TrustMore,
            true,
            false,
            Ask,
        ),
        (
            "outbound U In ask_more",
            Outbound,
            AskMore,
            false,
            true,
            Ask,
        ),
        ("outbound U In default", Outbound, Default, false, true, Ask),
        (
            "outbound U In trust_more",
            Outbound,
            TrustMore,
            false,
            true,
            Ask,
        ),
        (
            "outbound U Out trust_more",
            Outbound,
            TrustMore,
            false,
            false,
            Ask,
        ),
        (
            "destructive T In ask_more",
            Destructive,
            AskMore,
            true,
            true,
            Ask,
        ),
        (
            "destructive T In default",
            Destructive,
            Default,
            true,
            true,
            Ask,
        ),
        (
            "destructive T In trust_more",
            Destructive,
            TrustMore,
            true,
            true,
            Judged,
        ),
        (
            "destructive T Out ask_more",
            Destructive,
            AskMore,
            true,
            false,
            Ask,
        ),
        (
            "destructive T Out default",
            Destructive,
            Default,
            true,
            false,
            Ask,
        ),
        (
            "destructive T Out trust_more",
            Destructive,
            TrustMore,
            true,
            false,
            Ask,
        ),
        (
            "destructive U In ask_more",
            Destructive,
            AskMore,
            false,
            true,
            Ask,
        ),
        (
            "destructive U In default",
            Destructive,
            Default,
            false,
            true,
            Ask,
        ),
        (
            "destructive U In trust_more",
            Destructive,
            TrustMore,
            false,
            true,
            Ask,
        ),
        (
            "destructive U Out trust_more",
            Destructive,
            TrustMore,
            false,
            false,
            Ask,
        ),
    ];
    for (name, effect, s, trusted, inside, want) in rows {
        let request = with(effect, |r| {
            strictness(s)(r);
            // Reads never feed a sink; "untrusted" there is the planner alone.
            if !trusted {
                untrust_planner(r);
                if effect != Read {
                    untrust_recipient(r);
                }
            }
            if !inside {
                outside(r);
            }
        });
        let got = pdp.decide(&request);
        assert_eq!(cell(&got), want, "row {name}: {got:?}");
    }
}

#[test]
fn an_untrusted_planner_alone_breaks_the_trusted_cells() {
    let pdp = pdp();
    let got = pdp.decide(&with(Effect::UndoableWrite, untrust_planner));
    assert_eq!(cell(&got), Cell::Judged, "{got:?}");
    let got = pdp.decide(&with(Effect::Outbound, untrust_planner));
    assert_eq!(cell(&got), Cell::Ask, "{got:?}");
}
