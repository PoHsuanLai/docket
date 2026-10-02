//! Side conversations with a subagent.

mod support;

use agent_loop::*;
use almanac_core::EpisodeKind;
use support::*;

#[test]
fn a_side_conversation_ends_on_row_close_or_after_two_idle_minutes() {
    let told = |t: i64, text: &str| SideInput::Told {
        to: worker("t-9"),
        space: space("work"),
        turn: turn(t as u64, text, t),
    };
    let (table, effects) = side_step(
        SideTable::default(),
        told(100, "skip the newsletters"),
        &rules(),
    );
    assert!(
        matches!(effects.as_slice(), [SideEffect::Rederive { .. }]),
        "a goal change re-derives the policy"
    );
    assert_eq!(table.open.len(), 1);
    let (table, _) = side_step(table, told(150, "and the receipts"), &rules());
    assert_eq!(
        table.open[0].turns.len(),
        2,
        "a second turn extends the one conversation"
    );
    let (still, effects) = side_step(table.clone(), SideInput::Tick(at(150 + 119)), &rules());
    assert!(
        effects.is_empty() && still.open.len() == 1,
        "119 s is not yet idle"
    );
    let (gone, effects) = side_step(table.clone(), SideInput::Tick(at(150 + 120)), &rules());
    assert!(gone.open.is_empty());
    assert!(
        matches!(effects.as_slice(), [SideEffect::WriteEpisode { end: SideEnd::Idle, conv }] if conv.turns.len() == 2)
    );
    let (gone, effects) = side_step(table.clone(), SideInput::RowClosed(worker("t-9")), &rules());
    assert!(
        gone.open.is_empty()
            && matches!(
                effects.as_slice(),
                [SideEffect::WriteEpisode {
                    end: SideEnd::RowClosed,
                    ..
                }]
            )
    );
    let (same, effects) = side_step(table, SideInput::RowClosed(worker("t-77")), &rules());
    assert!(
        effects.is_empty() && same.open.len() == 1,
        "closing an unknown row does nothing"
    );
}

#[test]
fn a_side_episode_holds_the_persons_words_verbatim_and_no_narrative() {
    let conv = SideConv {
        with: worker("t-9"),
        space: space("work"),
        opened: at(100),
        last: at(150),
        turns: vec![turn(1, "skip the newsletters", 100)],
    };
    let episode = side_episode(&conv, vec![], vec![], at(300)).expect("a valid id");
    assert_eq!(episode.kind, EpisodeKind::Side);
    assert_eq!(episode.id.as_str(), "side-t-9-100");
    assert_eq!(episode.skeleton.asked[0].as_str(), "skip the newsletters");
    assert!(episode.narrative.is_none());
    assert_eq!(episode.agent, worker("t-9"));
}
