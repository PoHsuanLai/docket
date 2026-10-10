//! Speech output (voice.md §4.3) and `Prepare`: the commands that speak, hush and stop, and the
//! carrying out of the speech machine's effects. Synthesis runs in an inferd session task and
//! playback in its own task; this file decides, in the loop, what each report means.

use crate::command::{Caller, Reply};
use crate::engine::Core;
use crate::error::VoiceError;
use crate::link::{self, Link, LinkEvent};
use crate::playback::{Played, samples_of};
use crate::usage::UseSource;
use crate::warm::{Warm, tts_need};
use crate::{SpeechSkeleton, device::AudioDevice};
use porter_client::Transport;
use porter_core::capability::LanguageTag;
use porter_core::consent::Usage;
use porter_core::{DataClass, Tier};
use porter_infer::{
    ClientFrame, InferEvent, InferReply, InferRequest, ModelError, Readiness, SpeakRequest,
};
use voice_dbus::speech_path;
use voice_loop::{MicNow, SpeechEffect, SpeechEvent, SpeechPhase, SpeechState, speech_step};
use voice_wire::{SpeakWire, Speaking, SpeechEnd, VoiceFault, VoiceRefusal, VoiceUse};
use zbus::object_server::SignalEmitter;

/// One request to speak, as the loop holds it.
#[derive(Debug, Clone)]
pub(crate) struct Rec {
    pub n: u64,
    pub who: String,
    pub lang: LanguageTag,
    pub class: DataClass,
}

/// The speech side of the loop.
#[derive(Debug)]
pub(crate) struct Say {
    pub state: SpeechState,
    pub link: Option<Link>,
    /// Requests being spoken (merged requests finish together).
    pub active: Vec<Rec>,
    /// The request waiting for the mic or for a stop to drain (depth one).
    pub pending: Option<Rec>,
    /// Objects of finished requests, removed when the next one is made.
    pub done: Vec<u64>,
}

impl Say {
    pub fn new() -> Self {
        Self {
            state: SpeechState::quiet(),
            link: None,
            active: Vec::new(),
            pending: None,
            done: Vec::new(),
        }
    }
}

impl<D: AudioDevice + 'static, T: Transport + 'static, W: Warm, U: UseSource> Core<D, T, W, U>
where
    T::Session: 'static,
{
    pub(crate) fn speaking(&self) -> Speaking {
        match self.say.state.phase {
            SpeechPhase::Quiet => Speaking::Quiet,
            _ => Speaking::Speaking,
        }
    }

    /// The caller may speak: the shell, or the app attached to the utterance the text answers.
    fn may_speak(&self, caller: &Caller, wire: &SpeakWire) -> bool {
        if caller.is_shell() {
            return true;
        }
        let Some(utterance) = &wire.utterance else {
            return false;
        };
        self.utt.as_ref().is_some_and(|utt| {
            format!("u-{}", utt.n) == utterance.as_str()
                && utt
                    .attached
                    .as_ref()
                    .is_some_and(|(who, _)| *who == caller.unique)
        })
    }

    pub(crate) async fn speak(
        &mut self,
        caller: &Caller,
        wire: SpeakWire,
    ) -> Result<String, VoiceError> {
        if !self.may_speak(caller, &wire) {
            return Err(VoiceRefusal::NotAllowed.into());
        }
        if self.usage.now() == VoiceUse::Off {
            return Err(VoiceRefusal::Disabled.into());
        }
        self.counter += 1;
        let n = self.counter;
        let path = speech_path(n);
        let object = SpeechSkeleton::serving(n, self.handle.clone());
        for old in std::mem::take(&mut self.say.done) {
            let _ = self
                .conn
                .object_server()
                .remove::<SpeechSkeleton, _>(speech_path(old))
                .await;
        }
        self.conn
            .object_server()
            .at(path.as_str(), object)
            .await
            .map_err(VoiceError::from)?;
        let rec = Rec {
            n,
            who: caller.unique.clone(),
            lang: wire.lang.clone(),
            class: wire.class,
        };
        let joins = matches!(
            self.say.state.phase,
            SpeechPhase::Synth | SpeechPhase::Playing
        );
        if joins {
            self.say.active.push(rec);
        } else if let Some(replaced) = self.say.pending.replace(rec) {
            self.finish_one(&replaced, SpeechEnd::Hushed).await;
        }
        self.speech_event(SpeechEvent::Speak(wire)).await;
        Ok(path)
    }

    pub(crate) async fn hush(&mut self, caller: &Caller) -> Result<(), VoiceError> {
        if !caller.is_shell() {
            return Err(VoiceRefusal::NotAllowed.into());
        }
        self.hush_all().await;
        Ok(())
    }

    async fn hush_all(&mut self) {
        self.say.state.pending = None;
        if let Some(waiting) = self.say.pending.take() {
            self.finish_one(&waiting, SpeechEnd::Hushed).await;
        }
        self.speech_event(SpeechEvent::Hush).await;
    }

    pub(crate) async fn stop_speech(&mut self, n: u64, caller: &Caller) -> Result<(), VoiceError> {
        let mine = |rec: &Rec| rec.n == n && (caller.is_shell() || rec.who == caller.unique);
        if self.say.active.iter().any(mine) {
            self.hush_all().await;
            Ok(())
        } else if self.say.pending.as_ref().is_some_and(mine) {
            self.say.state.pending = None;
            if let Some(waiting) = self.say.pending.take() {
                self.finish_one(&waiting, SpeechEnd::Hushed).await;
            }
            Ok(())
        } else if self.say.done.contains(&n) && caller.is_shell() {
            Ok(())
        } else {
            Err(VoiceRefusal::NotAllowed.into())
        }
    }

    /// `Prepare`: answered by a task, so a cold engine never holds the loop.
    pub(crate) fn prepare(&mut self, caller: &Caller, reply: Reply<Result<String, VoiceError>>) {
        if !caller.is_shell() {
            let _ = reply.send(Err(VoiceRefusal::NotAllowed.into()));
            return;
        }
        let (warm, ready) = (self.warm.clone(), self.stt_ready.clone());
        tokio::spawn(async move {
            let now: Readiness = warm.warm().await;
            if let Ok(mut held) = ready.lock() {
                *held = now;
            }
            let _ = reply.send(Ok(now.slug().to_owned()));
        });
    }

    /// The mic opened or closed: the speech machine learns it.
    pub(crate) async fn speech_mic(&mut self, mic: MicNow) {
        self.speech_event(SpeechEvent::Mic(mic)).await;
    }

    pub(crate) async fn speech_event(&mut self, event: SpeechEvent) {
        let (state, effects) = speech_step(self.say.state.clone(), event);
        self.say.state = state;
        for effect in effects {
            self.speech_effect(effect).await;
        }
    }

    pub(crate) async fn speech_effect(&mut self, effect: SpeechEffect) {
        match effect {
            SpeechEffect::InferOpen(_) => {
                self.say.link = None;
                if let Some(rec) = self.say.pending.take() {
                    self.say.active = vec![rec];
                }
                self.status_changed().await;
            }
            SpeechEffect::SpeakSentence(sentence) => self.say_sentence(sentence.0),
            SpeechEffect::Play => {}
            SpeechEffect::StopSynth => {
                if let Some(link) = self.say.link.take() {
                    let _ = link.tx.send(ClientFrame::Cancel);
                }
            }
            SpeechEffect::Fade { ms } => self.player.stop(ms),
            SpeechEffect::Finished(end) => {
                for rec in std::mem::take(&mut self.say.active) {
                    self.finish_one(&rec, end).await;
                }
                if end == SpeechEnd::Done {
                    self.say.link = None;
                }
                self.status_changed().await;
            }
        }
    }

    fn say_sentence(&mut self, text: String) {
        let Some(rec) = self.say.active.last().cloned() else {
            return;
        };
        let request = SpeakRequest::new(text, rec.lang, rec.class, Usage::Interactive);
        let frame = ClientFrame::Request(InferRequest::Speak(request));
        match &self.say.link {
            Some(link) => {
                let _ = link.tx.send(frame);
            }
            None => {
                self.say.link = Some(link::spawn(
                    self.transport.clone(),
                    tts_need(),
                    rec.class,
                    Tier::Balanced,
                    frame,
                ));
            }
        }
    }

    /// The unicast `Finished` signal to the requester.
    async fn finish_one(&mut self, rec: &Rec, end: SpeechEnd) {
        self.say.done.push(rec.n);
        let Ok(body) = crate::wire::seal(&end) else {
            return;
        };
        let Ok(name) = zbus::names::BusName::try_from(rec.who.clone()) else {
            return;
        };
        let path = speech_path(rec.n);
        let Ok(emitter) = SignalEmitter::new(&self.conn, path.as_str()) else {
            return;
        };
        let _ = SpeechSkeleton::finished(&emitter.set_destination(name), &body).await;
    }

    /// What the synthesis session said.
    pub(crate) async fn tts_event(&mut self, event: Option<LinkEvent>) {
        let Some(event) = event else {
            self.say.link = None;
            return;
        };
        match event {
            LinkEvent::Ready => {}
            LinkEvent::Closed(fault) => self.speech_failed(fault).await,
            LinkEvent::Infer(event) => match *event {
                InferEvent::Waiting(r) => self.tts_ready = r,
                InferEvent::Spoken(out) => {
                    self.player.play(out.rate.0, samples_of(&out.pcm.0));
                    self.speech_event(SpeechEvent::Chunk).await;
                }
                InferEvent::Finished(InferReply::Spoke(_)) => {
                    self.speech_event(SpeechEvent::SentenceDone).await;
                    let draining = self.say.state.queue.is_empty()
                        && matches!(
                            self.say.state.phase,
                            SpeechPhase::Playing | SpeechPhase::Synth
                        );
                    if draining {
                        self.player.drain();
                    }
                }
                InferEvent::Finished(InferReply::Refused(why)) => {
                    self.speech_failed(VoiceFault::Refused(why)).await;
                }
                InferEvent::Finished(InferReply::Failed(why)) => {
                    self.speech_failed(VoiceFault::Engine(why)).await;
                }
                InferEvent::Finished(InferReply::Cancelled) => {}
                InferEvent::Finished(_) => {
                    self.speech_failed(VoiceFault::Engine(ModelError::Unreadable))
                        .await;
                }
                _ => {}
            },
        }
    }

    /// Synthesis or playback failed: everything being spoken ends `Failed`, and speech is quiet.
    async fn speech_failed(&mut self, fault: VoiceFault) {
        self.say.link = None;
        self.say.state.phase = SpeechPhase::Quiet;
        self.say.state.queue.clear();
        self.say.state.class = None;
        self.player.stop(voice_loop::FADE_MS);
        for rec in std::mem::take(&mut self.say.active) {
            self.finish_one(&rec, SpeechEnd::Failed(fault)).await;
        }
        self.status_changed().await;
    }

    /// What the playback task reported.
    pub(crate) async fn played(&mut self, played: Played) {
        match played {
            Played::Drained(generation) if generation == self.player.generation() => {
                self.speech_event(SpeechEvent::Drained).await;
            }
            Played::Drained(_) => {}
            Played::Failed => {
                self.speech_failed(VoiceFault::Engine(ModelError::Unreachable))
                    .await;
            }
        }
    }
}
