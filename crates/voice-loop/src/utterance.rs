//! One utterance (§4.2). Hold-to-talk or dictation: the mic opens on `Begin` and closes at the
//! end of the tail. The invariant is a property of this table alone.

use docket_core::VoiceIntent;
use porter_infer::{Readiness, ServedBy};
use serde::{Deserialize, Serialize};
use voice_wire::{
    CancelCause, HeardSegment, HeardTail, HeardText, Level, UtteranceEnd, VoiceEvent, VoiceFault,
    VoiceRefusal, VoiceTarget, VoiceUse,
};

/// How long the mic stays open after release, so the last syllable is not clipped.
pub const TAIL_MS: u32 = 250;

/// Where the utterance is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UtteranceState {
    /// Nothing.
    Idle,
    /// The mic is being opened and the engine session started.
    Opening,
    /// Listening.
    Listening,
    /// Released; the mic stays open for the tail.
    Tail,
    /// The mic is closed; waiting for the final transcript.
    Finishing,
}

/// Whether the microphone is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MicNow {
    /// Closed.
    Closed,
    /// Open.
    Open,
}

/// The mic is open exactly in `Opening`, `Listening` and `Tail`.
pub fn mic_of(state: UtteranceState) -> MicNow {
    match state {
        UtteranceState::Opening | UtteranceState::Listening | UtteranceState::Tail => MicNow::Open,
        UtteranceState::Idle | UtteranceState::Finishing => MicNow::Closed,
    }
}

/// Why a `Begin` is refused whatever the state, or `None` when voice is on.
pub fn refusal_for(enabled: VoiceUse) -> Option<VoiceRefusal> {
    match enabled {
        VoiceUse::On => None,
        VoiceUse::NeedsConsent => Some(VoiceRefusal::NeedsConsent),
        VoiceUse::Off => Some(VoiceRefusal::Disabled),
    }
}

/// Whether the speech engine can take audio now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineGate {
    /// Ready: forward audio.
    Ready,
    /// Cold: buffer it, never drop.
    Cold,
}

/// What happens to the utterance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum UtteranceEvent {
    /// `Voice1.Begin` from the shell.
    Begin {
        /// Whether voice may be used.
        enabled: VoiceUse,
        /// Ask or dictate.
        intent: VoiceIntent,
    },
    /// The capture stream runs.
    MicOpened,
    /// The capture stream failed.
    MicFailed(VoiceFault),
    /// One frame of audio arrived.
    Audio {
        /// Its level.
        level: Level,
        /// Whether the engine is ready for it.
        engine: EngineGate,
    },
    /// The engine reported its readiness.
    EngineWaiting(Readiness),
    /// An unstable tail arrived.
    Partial(HeardTail),
    /// A stable segment arrived.
    Final(HeardSegment),
    /// Dictation only: the endpointer saw silence. Handled as `Release`; the edge sends it only
    /// for dictation.
    EndpointSilence,
    /// The endpointer saw no speech at all.
    EndpointNoSpeech,
    /// The person released the key.
    Release,
    /// The tail timer fired.
    TailElapsed,
    /// The final transcript arrived.
    Finished(Transcript),
    /// Cancelled.
    Cancel(CancelCause),
    /// The engine failed or inferd refused.
    EngineFailed(VoiceFault),
    /// The shell named who may attach.
    Route(VoiceTarget),
    /// An app attached.
    Attach,
}

/// A final transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Transcript {
    /// Words.
    Text {
        /// What was said.
        text: HeardText,
        /// Who transcribed it.
        served: ServedBy,
    },
    /// Empty.
    Empty,
}

/// A short sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Earcon {
    /// Listening began.
    Begin,
    /// Listening ended.
    End,
}

/// What the daemon does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum UtteranceEffect {
    /// Open the capture stream (16 kHz mono S16, a physical source).
    OpenMic,
    /// Close it.
    CloseMic,
    /// Open the inferd speech session.
    InferOpen,
    /// Tell inferd the audio is over.
    SendEndOfAudio,
    /// Cancel the inferd request.
    SendCancel,
    /// Send this frame to inferd.
    Forward,
    /// Hold this frame (cap 10 s).
    Buffer,
    /// Tell clients the status changed.
    StatusChanged,
    /// Write to the attached fds.
    Emit(VoiceEvent),
    /// Play a sound.
    Earcon(Earcon),
    /// Wake after the tail.
    StartTail {
        /// How long, in milliseconds.
        ms: u32,
    },
    /// Wipe the buffers.
    Zeroize,
    /// Refuse the call.
    Refuse(VoiceRefusal),
    /// Remember who may attach.
    StoreRoute(VoiceTarget),
    /// Replay the committed segments and the last partial to the attached app.
    Replay,
}

use UtteranceEffect as E;
use UtteranceEvent as V;
use UtteranceState as S;

fn ended(end: UtteranceEnd) -> UtteranceEffect {
    E::Emit(VoiceEvent::Ended(end))
}

fn begin_effects() -> Vec<UtteranceEffect> {
    vec![E::OpenMic, E::InferOpen, E::StatusChanged]
}

fn stop(cause: UtteranceEnd) -> (UtteranceState, Vec<UtteranceEffect>) {
    (
        S::Idle,
        vec![
            E::CloseMic,
            E::SendCancel,
            ended(cause),
            E::Zeroize,
            E::StatusChanged,
        ],
    )
}

/// One transition. Total and pure.
pub fn utterance_step(
    state: UtteranceState,
    event: UtteranceEvent,
) -> (UtteranceState, Vec<UtteranceEffect>) {
    // The utterance is still live (the mic is closed in `Finishing`; see `mic_of`).
    let active = matches!(state, S::Opening | S::Listening | S::Tail | S::Finishing);
    match (state, event) {
        (
            S::Idle,
            V::Begin {
                enabled: VoiceUse::On,
                ..
            },
        ) => (S::Opening, begin_effects()),
        (
            _,
            V::Begin {
                enabled: VoiceUse::On,
                ..
            },
        ) => {
            // A second Begin supersedes the first.
            let mut effects = vec![
                E::CloseMic,
                E::SendCancel,
                ended(UtteranceEnd::Cancelled(CancelCause::Superseded)),
                E::Zeroize,
            ];
            effects.extend(begin_effects());
            (S::Opening, effects)
        }
        (
            s,
            V::Begin {
                enabled: VoiceUse::NeedsConsent,
                ..
            },
        ) => (s, vec![E::Refuse(VoiceRefusal::NeedsConsent)]),
        (
            s,
            V::Begin {
                enabled: VoiceUse::Off,
                ..
            },
        ) => (s, vec![E::Refuse(VoiceRefusal::Disabled)]),
        (S::Opening, V::MicOpened) => (
            S::Listening,
            vec![E::Emit(VoiceEvent::Opened), E::Earcon(Earcon::Begin)],
        ),
        (S::Opening, V::MicFailed(fault)) => (
            S::Idle,
            vec![
                ended(UtteranceEnd::Failed(fault)),
                E::CloseMic,
                E::StatusChanged,
            ],
        ),
        (S::Listening, V::Audio { level, engine }) => (
            S::Listening,
            vec![
                E::Emit(VoiceEvent::Level(level)),
                match engine {
                    EngineGate::Ready => E::Forward,
                    EngineGate::Cold => E::Buffer,
                },
            ],
        ),
        (S::Tail, V::Audio { engine, .. }) => (
            S::Tail,
            vec![match engine {
                EngineGate::Ready => E::Forward,
                EngineGate::Cold => E::Buffer,
            }],
        ),
        (s @ (S::Listening | S::Tail | S::Finishing), V::EngineWaiting(r)) => {
            (s, vec![E::Emit(VoiceEvent::Waiting(r))])
        }
        (s @ (S::Listening | S::Tail | S::Finishing), V::Partial(t)) => {
            (s, vec![E::Emit(VoiceEvent::Partial(t))])
        }
        (s @ (S::Listening | S::Tail | S::Finishing), V::Final(seg)) => {
            (s, vec![E::Emit(VoiceEvent::Committed(seg))])
        }
        (S::Listening, V::Release | V::EndpointSilence) => {
            (S::Tail, vec![E::StartTail { ms: TAIL_MS }])
        }
        (S::Tail, V::TailElapsed) => (
            S::Finishing,
            vec![
                E::CloseMic,
                E::SendEndOfAudio,
                E::Earcon(Earcon::End),
                E::StatusChanged,
            ],
        ),
        (S::Finishing, V::Finished(Transcript::Text { text, served })) if !text.0.is_empty() => (
            S::Idle,
            vec![ended(UtteranceEnd::Heard { text, served }), E::Zeroize],
        ),
        (S::Finishing, V::Finished(_)) => {
            (S::Idle, vec![ended(UtteranceEnd::NothingHeard), E::Zeroize])
        }
        (S::Finishing, V::EndpointNoSpeech) => {
            (S::Idle, vec![ended(UtteranceEnd::NothingHeard), E::Zeroize])
        }
        (_, V::Cancel(cause)) if active => stop(UtteranceEnd::Cancelled(cause)),
        (_, V::EngineFailed(fault)) if active => stop(UtteranceEnd::Failed(fault)),
        (s, V::Route(target)) => (s, vec![E::StoreRoute(target)]),
        (s, V::Attach) => (s, vec![E::Replay]),
        // Everything else is a late or stray event: no state change, no effect.
        (s, _) => (s, vec![]),
    }
}
