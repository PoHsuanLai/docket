//! The utterance table and the mic invariant.

use docket_core::VoiceIntent;
use porter_core::{AccountId, Locality, ModelId};
use porter_infer::{ModelError, Readiness, ServedBy};
use voice_loop::*;
use voice_wire::*;

fn served() -> ServedBy {
    ServedBy {
        account: AccountId::parse("local").expect("a"),
        model: ModelId::parse("m").expect("m"),
        locality: Locality::OnDevice,
    }
}
fn begin(enabled: VoiceUse) -> UtteranceEvent {
    UtteranceEvent::Begin {
        enabled,
        intent: VoiceIntent::Ask,
    }
}
fn heard(t: &str) -> Transcript {
    Transcript::Text {
        text: HeardText(t.into()),
        served: served(),
    }
}
fn ended(e: UtteranceEnd) -> UtteranceEffect {
    UtteranceEffect::Emit(VoiceEvent::Ended(e))
}

use UtteranceEffect as E;
use UtteranceEvent as V;
use UtteranceState as S;

type Row = (
    &'static str,
    UtteranceState,
    UtteranceEvent,
    UtteranceState,
    Vec<UtteranceEffect>,
);

#[test]
fn utterance_table() {
    let tail = HeardTail {
        text: HeardText("hel".into()),
    };
    let seg = HeardSegment {
        text: HeardText("hello".into()),
    };
    let rows: Vec<Row> = vec![
        (
            "begin opens the mic and the engine session",
            S::Idle,
            begin(VoiceUse::On),
            S::Opening,
            vec![E::OpenMic, E::InferOpen, E::StatusChanged],
        ),
        (
            "begin without consent refuses",
            S::Idle,
            begin(VoiceUse::NeedsConsent),
            S::Idle,
            vec![E::Refuse(VoiceRefusal::NeedsConsent)],
        ),
        (
            "begin while off refuses",
            S::Idle,
            begin(VoiceUse::Off),
            S::Idle,
            vec![E::Refuse(VoiceRefusal::Disabled)],
        ),
        (
            "mic opened announces and sounds",
            S::Opening,
            V::MicOpened,
            S::Listening,
            vec![E::Emit(VoiceEvent::Opened), E::Earcon(Earcon::Begin)],
        ),
        (
            "mic failure ends it",
            S::Opening,
            V::MicFailed(VoiceFault::MicDenied),
            S::Idle,
            vec![
                ended(UtteranceEnd::Failed(VoiceFault::MicDenied)),
                E::CloseMic,
                E::StatusChanged,
            ],
        ),
        (
            "audio with a ready engine is forwarded",
            S::Listening,
            V::Audio {
                level: Level(500),
                engine: EngineGate::Ready,
            },
            S::Listening,
            vec![E::Emit(VoiceEvent::Level(Level(500))), E::Forward],
        ),
        (
            "audio with a cold engine is buffered",
            S::Listening,
            V::Audio {
                level: Level(10),
                engine: EngineGate::Cold,
            },
            S::Listening,
            vec![E::Emit(VoiceEvent::Level(Level(10))), E::Buffer],
        ),
        (
            "audio in the tail shows no level",
            S::Tail,
            V::Audio {
                level: Level(10),
                engine: EngineGate::Ready,
            },
            S::Tail,
            vec![E::Forward],
        ),
        (
            "engine waiting is announced",
            S::Listening,
            V::EngineWaiting(Readiness::Loading),
            S::Listening,
            vec![E::Emit(VoiceEvent::Waiting(Readiness::Loading))],
        ),
        (
            "a partial is emitted",
            S::Listening,
            V::Partial(tail.clone()),
            S::Listening,
            vec![E::Emit(VoiceEvent::Partial(tail))],
        ),
        (
            "a final segment is committed",
            S::Listening,
            V::Final(seg.clone()),
            S::Listening,
            vec![E::Emit(VoiceEvent::Committed(seg))],
        ),
        (
            "release starts the tail",
            S::Listening,
            V::Release,
            S::Tail,
            vec![E::StartTail { ms: TAIL_MS }],
        ),
        (
            "dictation silence is a release",
            S::Listening,
            V::EndpointSilence,
            S::Tail,
            vec![E::StartTail { ms: TAIL_MS }],
        ),
        (
            "the tail ends by closing the mic and sending end of audio",
            S::Tail,
            V::TailElapsed,
            S::Finishing,
            vec![
                E::CloseMic,
                E::SendEndOfAudio,
                E::Earcon(Earcon::End),
                E::StatusChanged,
            ],
        ),
        (
            "a transcript ends it heard",
            S::Finishing,
            V::Finished(heard("hi")),
            S::Idle,
            vec![
                ended(UtteranceEnd::Heard {
                    text: HeardText("hi".into()),
                    served: served(),
                }),
                E::Zeroize,
            ],
        ),
        (
            "an empty transcript is nothing heard",
            S::Finishing,
            V::Finished(Transcript::Empty),
            S::Idle,
            vec![ended(UtteranceEnd::NothingHeard), E::Zeroize],
        ),
        (
            "empty text is nothing heard",
            S::Finishing,
            V::Finished(heard("")),
            S::Idle,
            vec![ended(UtteranceEnd::NothingHeard), E::Zeroize],
        ),
        (
            "no speech is nothing heard",
            S::Finishing,
            V::EndpointNoSpeech,
            S::Idle,
            vec![ended(UtteranceEnd::NothingHeard), E::Zeroize],
        ),
        (
            "escape cancels a listening utterance",
            S::Listening,
            V::Cancel(CancelCause::Escape),
            S::Idle,
            vec![
                E::CloseMic,
                E::SendCancel,
                ended(UtteranceEnd::Cancelled(CancelCause::Escape)),
                E::Zeroize,
                E::StatusChanged,
            ],
        ),
        (
            "cancel while finishing",
            S::Finishing,
            V::Cancel(CancelCause::Shell),
            S::Idle,
            vec![
                E::CloseMic,
                E::SendCancel,
                ended(UtteranceEnd::Cancelled(CancelCause::Shell)),
                E::Zeroize,
                E::StatusChanged,
            ],
        ),
        (
            "an engine failure closes the mic",
            S::Tail,
            V::EngineFailed(VoiceFault::Engine(ModelError::NotReady)),
            S::Idle,
            vec![
                E::CloseMic,
                E::SendCancel,
                ended(UtteranceEnd::Failed(VoiceFault::Engine(
                    ModelError::NotReady,
                ))),
                E::Zeroize,
                E::StatusChanged,
            ],
        ),
        (
            "a second begin supersedes the first",
            S::Listening,
            begin(VoiceUse::On),
            S::Opening,
            vec![
                E::CloseMic,
                E::SendCancel,
                ended(UtteranceEnd::Cancelled(CancelCause::Superseded)),
                E::Zeroize,
                E::OpenMic,
                E::InferOpen,
                E::StatusChanged,
            ],
        ),
        (
            "route is stored",
            S::Listening,
            V::Route(VoiceTarget::Shell),
            S::Listening,
            vec![E::StoreRoute(VoiceTarget::Shell)],
        ),
        (
            "attach replays",
            S::Listening,
            V::Attach,
            S::Listening,
            vec![E::Replay],
        ),
        (
            "cancel while idle does nothing",
            S::Idle,
            V::Cancel(CancelCause::Escape),
            S::Idle,
            vec![],
        ),
        (
            "stray audio while idle does nothing",
            S::Idle,
            V::Audio {
                level: Level(1),
                engine: EngineGate::Ready,
            },
            S::Idle,
            vec![],
        ),
        (
            "a release while opening is ignored",
            S::Opening,
            V::Release,
            S::Opening,
            vec![],
        ),
    ];
    for (name, from, event, to, effects) in rows {
        assert_eq!(utterance_step(from, event), (to, effects), "case: {name}");
    }
}

fn alphabet() -> Vec<UtteranceEvent> {
    vec![
        begin(VoiceUse::On),
        begin(VoiceUse::Off),
        begin(VoiceUse::NeedsConsent),
        V::MicOpened,
        V::MicFailed(VoiceFault::MicUnavailable),
        V::Audio {
            level: Level(1),
            engine: EngineGate::Cold,
        },
        V::EngineWaiting(Readiness::Loading),
        V::Partial(HeardTail {
            text: HeardText("a".into()),
        }),
        V::EndpointSilence,
        V::EndpointNoSpeech,
        V::Release,
        V::TailElapsed,
        V::Finished(Transcript::Empty),
        V::Cancel(CancelCause::OtherInput),
        V::EngineFailed(VoiceFault::NoModel),
    ]
}

/// The mic as the effects leave it: the last of OpenMic and CloseMic wins.
fn mic_after(mic: MicNow, effects: &[UtteranceEffect]) -> MicNow {
    effects.iter().fold(mic, |m, e| match e {
        E::OpenMic => MicNow::Open,
        E::CloseMic => MicNow::Closed,
        _ => m,
    })
}

fn walk(depth: usize, state: UtteranceState, mic: MicNow, visited: &mut usize) {
    if depth == 0 {
        return;
    }
    for event in alphabet() {
        let opens = matches!(
            event,
            V::Begin {
                enabled: VoiceUse::On,
                ..
            }
        );
        let (next, effects) = utterance_step(state, event);
        *visited += 1;
        let after = mic_after(mic, &effects);
        assert_eq!(
            after,
            mic_of(next),
            "effects leave the mic out of step with {next:?}: {effects:?}"
        );
        assert!(
            opens || !effects.contains(&E::OpenMic),
            "OpenMic without an accepted Begin"
        );
        walk(depth - 1, next, after, visited);
    }
}

#[test]
fn mic_open_iff_open_states() {
    let mut visited = 0;
    walk(4, UtteranceState::Idle, MicNow::Closed, &mut visited);
    assert_eq!(visited, 15 + 15 * 15 + 15 * 15 * 15 + 15 * 15 * 15 * 15);
    for (state, mic) in [
        (S::Idle, MicNow::Closed),
        (S::Opening, MicNow::Open),
        (S::Listening, MicNow::Open),
        (S::Tail, MicNow::Open),
        (S::Finishing, MicNow::Closed),
    ] {
        assert_eq!(mic_of(state), mic);
    }
}

#[test]
fn the_refusal_for_a_setting_matches_what_the_table_refuses() {
    for enabled in [VoiceUse::Off, VoiceUse::NeedsConsent, VoiceUse::On] {
        let (state, effects) = utterance_step(UtteranceState::Idle, begin(enabled));
        let refused = effects.into_iter().find_map(|e| match e {
            UtteranceEffect::Refuse(r) => Some(r),
            _ => None,
        });
        assert_eq!(refused, refusal_for(enabled), "{enabled:?}");
        assert_eq!(state == UtteranceState::Idle, refused.is_some());
    }
}

#[test]
fn a_second_begin_while_off_keeps_the_state_and_refuses() {
    let (state, effects) = utterance_step(UtteranceState::Listening, begin(VoiceUse::Off));
    assert_eq!(state, UtteranceState::Listening);
    assert_eq!(
        effects,
        vec![UtteranceEffect::Refuse(VoiceRefusal::Disabled)]
    );
}

#[test]
fn milliseconds_become_samples_at_the_capture_rate() {
    assert_eq!(samples_in(1000), CAPTURE_RATE as usize);
    assert_eq!(samples_in(TAIL_MS), 4_000);
    assert_eq!(BUFFER_SAMPLES, samples_in(10_000));
}
