//! One utterance: the commands that begin, route, attach, release and cancel it, and the carrying
//! out of the utterance machine's effects (voice.md §4.2). The mic is open exactly while the
//! machine says so: capture is opened by `OpenMic` and dropped by `CloseMic`, and nothing else
//! touches it.

use crate::command::Caller;
use crate::device::{
    AudioDevice, CaptureFormat, DeviceError, SNAPSHOT_BUDGET, choose_capture, snapshot_before,
};
use crate::engine::{Core, EndOfAudio, SttLink, Utt};
use crate::error::VoiceError;
use crate::link;
use crate::names::{VOICE_PATH, utterance_path};
use crate::playback;
use crate::sink::EventSink;
use crate::usage::UseSource;
use crate::warm::{Warm, stt_need};
use crate::{UtteranceSkeleton, VoiceSkeleton};
use docket_core::VoiceIntent;
use porter_client::Transport;
use porter_core::consent::Usage;
use porter_core::{DataClass, Tier, UnixSeconds};
use porter_infer::{
    AudioFrame, AudioRate, Base64Bytes, ClientFrame, InferRequest, LangPick, TranscribeBegin,
    TranscribeMode,
};
use std::os::fd::OwnedFd;
use voice_loop::{
    CAPTURE_RATE, MicNow, PcmBuffer, PushOutcome, UtteranceEffect, UtteranceEvent, UtteranceState,
    begin_utterance, mic_of, refusal_for, samples_in, utterance_step,
};
use voice_wire::{
    CancelCause, HeardText, MicState, UtteranceEnd, VoiceBegin, VoiceEvent, VoiceFault,
    VoiceRefusal, VoiceStatus, VoiceTarget,
};
use zbus::object_server::SignalEmitter;

fn fault_of(error: DeviceError) -> VoiceFault {
    match error {
        DeviceError::Denied => VoiceFault::MicDenied,
        DeviceError::NoSource | DeviceError::Closed | DeviceError::TimedOut => {
            VoiceFault::MicUnavailable
        }
    }
}

fn bytes_of(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

impl<D: AudioDevice + 'static, T: Transport + 'static, W: Warm, U: UseSource> Core<D, T, W, U>
where
    T::Session: 'static,
{
    pub(crate) fn now() -> UnixSeconds {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        UnixSeconds(secs)
    }

    pub(crate) fn status_text(&self) -> String {
        let mic = match &self.utt {
            Some(utt) if mic_of(utt.state) == MicNow::Open => MicState::Open {
                since: utt.since,
                intent: utt.intent,
            },
            _ => MicState::Closed,
        };
        let status = VoiceStatus {
            mic,
            speaking: self.speaking(),
            stt: self
                .stt_ready
                .lock()
                .map_or(porter_infer::Readiness::Unavailable, |r| *r),
            tts: self.tts_ready,
            enabled: self.usage.now(),
        };
        crate::wire::seal(&status).unwrap_or_default()
    }

    /// Tells every listener the mic or the speech started or stopped (no content).
    pub(crate) async fn status_changed(&self) {
        let Ok(emitter) = SignalEmitter::new(&self.conn, VOICE_PATH) else {
            return;
        };
        let _ = VoiceSkeleton::status_changed(&emitter, &self.status_text()).await;
    }

    pub(crate) async fn begin(
        &mut self,
        caller: &Caller,
        begin: VoiceBegin,
    ) -> Result<(String, OwnedFd), VoiceError> {
        if !caller.is_shell() {
            return Err(VoiceRefusal::NotAllowed.into());
        }
        let enabled = self.usage.now();
        let probe = UtteranceEvent::Begin {
            enabled,
            intent: begin.intent,
        };
        if let Some(refusal) = refusal_for(enabled) {
            return Err(refusal.into());
        }
        let Ok(found) = snapshot_before(&*self.device, tokio::time::sleep(SNAPSHOT_BUDGET)).await
        else {
            return Err(VoiceRefusal::MicUnavailable.into());
        };
        let Some(node) = choose_capture(
            &found.sources,
            found.default.as_deref(),
            self.input.as_deref(),
        )
        .map(|chosen| chosen.node.clone()) else {
            return Err(VoiceRefusal::MicUnavailable.into());
        };
        let (sink, fd) = EventSink::open().map_err(|e| VoiceError::Malformed(e.to_string()))?;
        self.utt_event(UtteranceEvent::Cancel(CancelCause::Superseded))
            .await;
        self.counter += 1;
        let n = self.counter;
        let path = utterance_path(n);
        let object = UtteranceSkeleton::serving(n, self.handle.clone());
        self.forget_utterance_object().await;
        self.conn
            .object_server()
            .at(path.as_str(), object)
            .await
            .map_err(VoiceError::from)?;
        self.node = Some(node);
        self.utt = Some(Utt {
            n,
            caller: caller.unique.clone(),
            intent: begin.intent,
            since: Self::now(),
            state: UtteranceState::Idle,
            sink,
            attached: None,
            route: None,
            committed: Vec::new(),
            partial: None,
            ended: None,
            carry: Vec::new(),
            pcm: PcmBuffer::new(),
            tail_left: None,
            sent: 0,
            engine: SttLink::Cold {
                end: EndOfAudio::NotYet,
            },
            dictation: (begin.intent == VoiceIntent::Dictate).then(crate::hear::Dictation::new),
        });
        let (speech, state, steps) =
            begin_utterance(self.say.state.clone(), UtteranceState::Idle, probe);
        self.say.state = speech;
        if let Some(utt) = self.utt.as_mut() {
            utt.state = state;
        }
        self.run_steps(steps).await;
        Ok((path, fd))
    }

    /// The previous utterance's object leaves the bus when the next begins.
    async fn forget_utterance_object(&self) {
        if let Some(old) = self
            .utt
            .as_ref()
            .filter(|u| mic_of(u.state) == MicNow::Closed)
        {
            let _ = self
                .conn
                .object_server()
                .remove::<UtteranceSkeleton, _>(utterance_path(old.n))
                .await;
        }
    }

    pub(crate) async fn route(
        &mut self,
        n: u64,
        caller: &Caller,
        target: VoiceTarget,
    ) -> Result<(), VoiceError> {
        self.owned_utt(n, caller)?;
        self.utt_event(UtteranceEvent::Route(target)).await;
        Ok(())
    }

    pub(crate) async fn attach(&mut self, n: u64, caller: &Caller) -> Result<OwnedFd, VoiceError> {
        let Some(utt) = self.utt.as_ref().filter(|u| u.n == n) else {
            return Err(VoiceRefusal::NotAllowed.into());
        };
        let Some(VoiceTarget::App { app, .. }) = utt.route.clone() else {
            return Err(VoiceRefusal::NotAllowed.into());
        };
        if !self.peers.owns(&caller.unique, &app).await {
            return Err(VoiceRefusal::NotAllowed.into());
        }
        if utt.attached.is_some() {
            return Err(VoiceRefusal::Busy.into());
        }
        let (sink, fd) = EventSink::open().map_err(|e| VoiceError::Malformed(e.to_string()))?;
        if let Some(utt) = self.utt.as_mut() {
            utt.attached = Some((caller.unique.clone(), sink));
        }
        self.utt_event(UtteranceEvent::Attach).await;
        Ok(fd)
    }

    pub(crate) async fn release(&mut self, n: u64, caller: &Caller) -> Result<(), VoiceError> {
        self.owned_utt(n, caller)?;
        self.utt_event(UtteranceEvent::Release).await;
        Ok(())
    }

    pub(crate) async fn cancel(
        &mut self,
        n: u64,
        caller: &Caller,
        cause: CancelCause,
    ) -> Result<(), VoiceError> {
        let Some(utt) = self.utt.as_ref().filter(|u| u.n == n) else {
            return Err(VoiceRefusal::NotAllowed.into());
        };
        let attached = utt
            .attached
            .as_ref()
            .is_some_and(|(who, _)| *who == caller.unique);
        if utt.caller != caller.unique && !attached {
            return Err(VoiceRefusal::NotAllowed.into());
        }
        self.utt_event(UtteranceEvent::Cancel(cause)).await;
        Ok(())
    }

    /// Runs one event through the utterance machine and carries out what it asks, then lets the
    /// speech machine know if the mic changed.
    pub(crate) async fn utt_event(&mut self, event: UtteranceEvent) {
        let mut work = std::collections::VecDeque::from([event]);
        while let Some(event) = work.pop_front() {
            let Some(utt) = self.utt.as_mut() else { return };
            let before = mic_of(utt.state);
            let (state, effects) = utterance_step(utt.state, event);
            utt.state = state;
            for effect in effects {
                if let Some(follow) = self.effect(effect).await {
                    work.push_back(follow);
                }
            }
            let after = self
                .utt
                .as_ref()
                .map_or(MicNow::Closed, |u| mic_of(u.state));
            if before != after {
                self.speech_mic(after).await;
            }
        }
    }

    /// Carries out the steps `begin_utterance` ordered (speech first, then the utterance).
    async fn run_steps(&mut self, steps: Vec<voice_loop::Step>) {
        let mut follow = Vec::new();
        for step in steps {
            match step {
                voice_loop::Step::Speech(effect) => self.speech_effect(effect).await,
                voice_loop::Step::Utterance(effect) => {
                    follow.extend(self.effect(effect).await);
                }
            }
        }
        for event in follow {
            self.utt_event(event).await;
        }
    }

    /// One effect; it may answer with the event that follows from doing it.
    async fn effect(&mut self, effect: UtteranceEffect) -> Option<UtteranceEvent> {
        match effect {
            UtteranceEffect::OpenMic => return Some(self.open_mic().await),
            UtteranceEffect::CloseMic => self.capture = None,
            UtteranceEffect::InferOpen => self.open_stt(),
            UtteranceEffect::SendEndOfAudio => self.end_of_audio(),
            UtteranceEffect::SendCancel => {
                if let Some(link) = self.stt.take() {
                    let _ = link.tx.send(ClientFrame::Cancel);
                }
            }
            UtteranceEffect::Forward => self.forward(),
            UtteranceEffect::Buffer => {
                let cur = std::mem::take(&mut self.cur);
                if let Some(utt) = self.utt.as_mut() {
                    // Past ten seconds the oldest audio goes (voice.md §4.2).
                    let _: PushOutcome = utt.pcm.push(&cur);
                }
            }
            UtteranceEffect::StatusChanged => self.status_changed().await,
            UtteranceEffect::Emit(event) => self.emit(event).await,
            UtteranceEffect::Earcon(which) => {
                if self.earcons == crate::config::Earcons::On {
                    playback::earcon(self.device.clone(), which);
                }
            }
            UtteranceEffect::StartTail { ms } => {
                if let Some(utt) = self.utt.as_mut() {
                    utt.tail_left = Some(samples_in(ms));
                }
            }
            UtteranceEffect::Zeroize => {
                self.cur.clear();
                if let Some(utt) = self.utt.as_mut() {
                    utt.pcm.wipe();
                    utt.carry.clear();
                }
            }
            UtteranceEffect::Refuse(_) => {}
            UtteranceEffect::StoreRoute(target) => {
                if let Some(utt) = self.utt.as_mut() {
                    utt.route = Some(target);
                }
            }
            UtteranceEffect::Replay => self.replay(),
        }
        None
    }

    async fn open_mic(&mut self) -> UtteranceEvent {
        let Some(node) = self.node.clone() else {
            return UtteranceEvent::MicFailed(VoiceFault::MicUnavailable);
        };
        match self
            .device
            .open_capture(&node, CaptureFormat { rate: CAPTURE_RATE })
            .await
        {
            Ok(stream) => {
                self.capture = Some(stream);
                UtteranceEvent::MicOpened
            }
            Err(error) => UtteranceEvent::MicFailed(fault_of(error)),
        }
    }

    fn open_stt(&mut self) {
        let first = ClientFrame::Request(InferRequest::Transcribe(TranscribeBegin {
            mode: TranscribeMode::Streaming,
            lang: LangPick::Auto,
            rate: AudioRate(CAPTURE_RATE),
            usage: Usage::Interactive,
        }));
        self.stt = Some(link::spawn(
            self.transport.clone(),
            stt_need(),
            DataClass::Voice,
            Tier::Balanced,
            first,
        ));
    }

    pub(crate) fn end_of_audio(&mut self) {
        let Some(utt) = self.utt.as_mut() else { return };
        match (&self.stt, utt.engine) {
            (Some(link), SttLink::Ready) => {
                let _ = link.tx.send(ClientFrame::EndOfAudio);
            }
            _ => {
                utt.engine = SttLink::Cold {
                    end: EndOfAudio::Pending,
                }
            }
        }
    }

    /// Sends `samples` to inferd in frames of at most a second, in order.
    pub(crate) fn send_audio(&mut self, samples: &[i16]) {
        let (Some(link), Some(utt)) = (&self.stt, self.utt.as_mut()) else {
            return;
        };
        for piece in samples.chunks(samples_in(1000)) {
            let frame = AudioFrame {
                at: utt.sent,
                pcm: Base64Bytes(bytes_of(piece)),
            };
            utt.sent += piece.len() as u64;
            let _ = link.tx.send(ClientFrame::Audio(frame));
        }
    }

    fn forward(&mut self) {
        let cur = std::mem::take(&mut self.cur);
        self.send_audio(&cur);
    }

    async fn emit(&mut self, event: VoiceEvent) {
        let Some(utt) = self.utt.as_mut() else { return };
        match &event {
            VoiceEvent::Partial(tail) => utt.partial = Some(tail.clone()),
            VoiceEvent::Committed(segment) => {
                utt.partial = None;
                utt.committed.push(segment.clone());
            }
            VoiceEvent::Ended(end) => utt.ended = Some(end.clone()),
            _ => {}
        }
        utt.sink.send(&event);
        if let Some((_, sink)) = &utt.attached {
            sink.send(&event);
        }
        if let VoiceEvent::Ended(end) = event {
            self.ended(end).await;
        }
    }

    /// The unicast `Ended` signal (no text) to the Begin caller and the attached app, and the
    /// end of the inferd session.
    async fn ended(&mut self, end: UtteranceEnd) {
        self.stt = None;
        let Some(utt) = self.utt.as_ref() else { return };
        let end = match end {
            UtteranceEnd::Heard { served, .. } => UtteranceEnd::Heard {
                text: HeardText(String::new()),
                served,
            },
            other => other,
        };
        let Ok(body) = crate::wire::seal(&end) else {
            return;
        };
        let path = utterance_path(utt.n);
        let recipients: Vec<String> = std::iter::once(utt.caller.clone())
            .chain(utt.attached.iter().map(|(who, _)| who.clone()))
            .collect();
        for who in recipients {
            let Ok(name) = zbus::names::BusName::try_from(who) else {
                continue;
            };
            let Ok(emitter) = SignalEmitter::new(&self.conn, path.as_str()) else {
                continue;
            };
            let _ = UtteranceSkeleton::ended(&emitter.set_destination(name), &body).await;
        }
    }

    /// The committed segments, the last partial and the end if it came, to the attached app.
    fn replay(&self) {
        let Some(utt) = &self.utt else { return };
        let Some((_, sink)) = &utt.attached else {
            return;
        };
        if mic_of(utt.state) == MicNow::Open || utt.ended.is_some() {
            sink.send(&VoiceEvent::Opened);
        }
        utt.committed
            .iter()
            .for_each(|s| sink.send(&VoiceEvent::Committed(s.clone())));
        if let Some(tail) = &utt.partial {
            sink.send(&VoiceEvent::Partial(tail.clone()));
        }
        if let Some(end) = &utt.ended {
            sink.send(&VoiceEvent::Ended(end.clone()));
        }
    }
}
