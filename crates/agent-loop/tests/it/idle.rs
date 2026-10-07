//! The idle pass.

use crate::support::*;
use agent_loop::*;
use almanac_core::EpisodeId;

fn job(id: &str, read: ReadUntrusted) -> EpisodeJob {
    EpisodeJob {
        episode: EpisodeId::parse(id).expect("id"),
        read,
    }
}

#[test]
fn the_idle_pass_waits_for_quiet_runs_one_narrative_and_yields_to_the_person() {
    let rules = rules();
    let state = IdleState::quiet_since(at(0));
    let (state, effects) = idle_step(
        state,
        IdleInput::EpisodeClosed(job("t-1", ReadUntrusted::Yes)),
        &rules,
    );
    assert!(effects.is_empty());
    let (state, effects) = idle_step(
        state,
        IdleInput::EpisodeClosed(job("t-2", ReadUntrusted::No)),
        &rules,
    );
    assert!(effects.is_empty());
    let (state, effects) = idle_step(state, IdleInput::Tick(at(29)), &rules);
    assert!(effects.is_empty(), "29 s of quiet is not 30");
    let (state, effects) = idle_step(state, IdleInput::Tick(at(30)), &rules);
    let first = NarrativeJob {
        episode: EpisodeId::parse("t-1").expect("id"),
        via: NarrativeVia::Reader,
    };
    assert_eq!(
        effects,
        [IdleEffect::Start(first.clone())],
        "a task that read untrusted content narrates in the reader"
    );
    // The person comes back: the stream is cancelled and the job waits at the front.
    let (state, effects) = idle_step(state, IdleInput::InteractiveStarted, &rules);
    assert_eq!(effects, [IdleEffect::Cancel(first.episode.clone())]);
    assert_eq!(state.phase, IdlePhase::Busy);
    assert_eq!(state.queue[0].episode, first.episode);
    let (state, _) = idle_step(state, IdleInput::InteractiveEnded(at(100)), &rules);
    let (state, effects) = idle_step(state, IdleInput::Tick(at(130)), &rules);
    assert_eq!(effects, [IdleEffect::Start(first.clone())]);
    let (state, effects) = idle_step(
        state,
        IdleInput::NarrativeDone(first.episode.clone()),
        &rules,
    );
    assert_eq!(effects, [IdleEffect::ProposeFacts(first.episode)]);
    let (_, effects) = idle_step(state, IdleInput::Tick(at(131)), &rules);
    let second = NarrativeJob {
        episode: EpisodeId::parse("t-2").expect("id"),
        via: NarrativeVia::Companion,
    };
    assert_eq!(
        effects,
        [IdleEffect::Start(second)],
        "the next one needs no new wait while the person is away"
    );
}

#[test]
fn a_failed_narrative_waits_for_the_person_to_come_and_go() {
    let rules = rules();
    let state = IdleState {
        phase: IdlePhase::Quiet { since: at(0) },
        queue: vec![job("t-1", ReadUntrusted::No)],
    };
    let (state, _) = idle_step(state, IdleInput::Tick(at(40)), &rules);
    let (state, effects) = idle_step(
        state,
        IdleInput::NarrativeFailed(EpisodeId::parse("t-1").expect("id")),
        &rules,
    );
    assert!(effects.is_empty());
    let (_, effects) = idle_step(state.clone(), IdleInput::Tick(at(500)), &rules);
    assert!(effects.is_empty(), "no retry loop");
    assert_eq!(state.queue.len(), 1);
}
