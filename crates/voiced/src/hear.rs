//! Hearing: the capture frames that arrive while the mic is open (cut into 32 ms frames, levelled,
//! counted against the tail, watched for the silence that ends a dictation) and what inferd says
//! back (partials, segments, the transcript, a refusal).

use crate::device::AudioDevice;
use crate::engine::{Core, EndOfAudio, SttLink};
use crate::link::LinkEvent;
use crate::usage::UseSource;
use crate::warm::Warm;
use porter_client::Transport;
use porter_infer::{HeardDelta, InferEvent, InferReply, ModelError};
use speech_provider::{Frame512, SampleIndex, VoiceActivity, Voiced};
use speech_vad::{Endpoint, EndpointParams, EnergyGate, EnergyGateParams, endpoint, level_of};
use voice_loop::{EngineGate, Transcript, UtteranceEvent, UtteranceState};
use voice_wire::{HeardSegment, HeardTail, HeardText, Level, VoiceFault};

/// The silence detector of a dictation: it ends the utterance after the silence it is told.
#[derive(Debug)]
pub(crate) struct Dictation {
    gate: EnergyGate,
    state: Endpoint,
    at: u64,
}

impl Dictation {
    pub fn new() -> Self {
        Self {
            gate: EnergyGate::new(EnergyGateParams::default()),
            state: Endpoint::Waiting,
            at: 0,
        }
    }

    /// Whether this frame ended it (after speech or never any).
    pub(crate) fn ended(&mut self, frame: &Frame512) -> bool {
        let (voiced, _): (Voiced, _) = self.gate.push(frame);
        self.state = endpoint(
            self.state,
            voiced,
            SampleIndex(self.at),
            &EndpointParams::default(),
        );
        self.at += 512;
        matches!(self.state, Endpoint::Ended(_))
    }
}

impl<D: AudioDevice + 'static, T: Transport + 'static, W: Warm, U: UseSource> Core<D, T, W, U>
where
    T::Session: 'static,
{
    /// A capture frame (or the end of the stream) arrived.
    pub(crate) async fn audio(&mut self, frame: Option<Vec<i16>>) {
        let Some(samples) = frame else {
            self.capture = None;
            let state = self.utt.as_ref().map(|u| u.state);
            match state {
                Some(UtteranceState::Tail) => self.utt_event(UtteranceEvent::TailElapsed).await,
                Some(UtteranceState::Opening | UtteranceState::Listening) => {
                    self.utt_event(UtteranceEvent::EngineFailed(VoiceFault::MicUnavailable))
                        .await;
                }
                _ => {}
            }
            return;
        };
        if let Some(utt) = self.utt.as_mut() {
            utt.carry.extend(samples);
        }
        loop {
            let Some(utt) = self.utt.as_mut() else { return };
            if utt.carry.len() < 512 {
                return;
            }
            let frame: Vec<i16> = utt.carry.drain(..512).collect();
            self.one_frame(frame).await;
        }
    }

    async fn one_frame(&mut self, frame: Vec<i16>) {
        let Ok(framed) = Frame512::new(&frame) else {
            return;
        };
        let level = Level(level_of(&framed).0);
        let engine = self
            .utt
            .as_ref()
            .map_or(EngineGate::Cold, |u| u.engine.gate());
        self.cur = frame;
        self.utt_event(UtteranceEvent::Audio { level, engine })
            .await;
        let Some(utt) = self.utt.as_mut() else { return };
        let silent = match (utt.state, utt.dictation.as_mut()) {
            (UtteranceState::Listening, Some(dictation)) => dictation.ended(&framed),
            _ => false,
        };
        let tail_over = match (utt.state, utt.tail_left) {
            (UtteranceState::Tail, Some(left)) => {
                let left = left.saturating_sub(512);
                utt.tail_left = Some(left);
                left == 0
            }
            _ => false,
        };
        if silent {
            self.utt_event(UtteranceEvent::EndpointSilence).await;
        }
        if tail_over {
            self.utt_event(UtteranceEvent::TailElapsed).await;
        }
    }

    /// What the inferd speech-to-text session said.
    pub(crate) async fn stt_event(&mut self, event: Option<LinkEvent>) {
        let Some(event) = event else {
            self.stt = None;
            return;
        };
        match event {
            LinkEvent::Ready => self.engine_ready().await,
            LinkEvent::Closed(fault) => {
                self.stt = None;
                self.utt_event(UtteranceEvent::EngineFailed(fault)).await;
            }
            LinkEvent::Infer(event) => {
                if let Some(next) = self.heard(*event) {
                    self.utt_event(next).await;
                }
            }
        }
    }

    async fn engine_ready(&mut self) {
        let Some(utt) = self.utt.as_mut() else { return };
        let end = match utt.engine {
            SttLink::Cold { end } => end,
            SttLink::Ready => EndOfAudio::NotYet,
        };
        utt.engine = SttLink::Ready;
        let held = utt.pcm.take();
        self.send_audio(&held);
        if end == EndOfAudio::Pending {
            self.end_of_audio();
        }
    }

    /// An inferd event as the utterance machine's input, if it is one.
    fn heard(&mut self, event: InferEvent) -> Option<UtteranceEvent> {
        match event {
            InferEvent::Waiting(r) => {
                if let Ok(mut ready) = self.stt_ready.lock() {
                    *ready = r;
                }
                Some(UtteranceEvent::EngineWaiting(r))
            }
            InferEvent::Heard(HeardDelta::Partial { text, .. }) => {
                Some(UtteranceEvent::Partial(HeardTail {
                    text: HeardText(text),
                }))
            }
            InferEvent::Heard(HeardDelta::Final { text, .. }) => {
                Some(UtteranceEvent::Final(HeardSegment {
                    text: HeardText(text),
                }))
            }
            InferEvent::Finished(InferReply::Transcribed(reply)) => {
                Some(UtteranceEvent::Finished(Transcript::Text {
                    text: HeardText(reply.text),
                    served: reply.served,
                }))
            }
            InferEvent::Finished(InferReply::Refused(why)) => {
                Some(UtteranceEvent::EngineFailed(VoiceFault::Refused(why)))
            }
            InferEvent::Finished(InferReply::Failed(why)) => {
                Some(UtteranceEvent::EngineFailed(VoiceFault::Engine(why)))
            }
            InferEvent::Finished(InferReply::Cancelled) => None,
            InferEvent::Finished(_) => Some(UtteranceEvent::EngineFailed(VoiceFault::Engine(
                ModelError::Unreadable,
            ))),
            _ => None,
        }
    }
}
