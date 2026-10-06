//! The daemon's one loop. Everything that changes state happens here, in one task: bus commands,
//! capture frames, inferd events and playback reports are all inputs to it, and the two pure
//! machines of `voice-loop` decide what each input does. A command is answered as soon as it is
//! decided; slow things (opening the engine, warming it, playing) run in their own tasks and come
//! back as events, so a cold engine never delays a `Begin`.

use crate::command::{Caller, Command};
use crate::config::Earcons;
use crate::device::{AudioDevice, AudioNode, CaptureStream};
use crate::error::VoiceError;
use crate::link::{Link, LinkEvent};
use crate::peer::Peers;
use crate::playback::{Played, Player};
use crate::sink::EventSink;
use crate::talk::Say;
use crate::usage::UseSource;
use crate::warm::Warm;
use docket_core::VoiceIntent;
use porter_client::Transport;
use porter_infer::Readiness;
use std::future::pending;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use voice_loop::{PcmBuffer, UtteranceState};
use voice_wire::{HeardSegment, HeardTail, UtteranceEnd, VoiceTarget};

/// What the loop knows of the one utterance it holds (the last one stays until the next begins,
/// so a late `Release` or `Attach` finds its object).
#[derive(Debug)]
pub(crate) struct Utt {
    pub n: u64,
    pub caller: String,
    pub intent: VoiceIntent,
    pub since: porter_core::UnixSeconds,
    pub state: UtteranceState,
    pub sink: EventSink,
    pub attached: Option<(String, EventSink)>,
    pub route: Option<VoiceTarget>,
    pub committed: Vec<HeardSegment>,
    pub partial: Option<HeardTail>,
    pub ended: Option<UtteranceEnd>,
    pub carry: Vec<i16>,
    pub pcm: PcmBuffer,
    pub tail_left: Option<usize>,
    pub sent: u64,
    pub end_pending: bool,
    pub ready: bool,
    pub dictation: Option<crate::hear::Dictation>,
}

/// The loop's state.
pub(crate) struct Core<D: AudioDevice + 'static, T: Transport + 'static, W: Warm, U: UseSource>
where
    T::Session: 'static,
{
    pub conn: zbus::Connection,
    pub handle: crate::command::Handle,
    pub peers: Peers,
    pub device: Arc<D>,
    pub transport: Arc<T>,
    pub warm: Arc<W>,
    pub usage: U,
    pub earcons: Earcons,
    pub counter: u64,
    pub utt: Option<Utt>,
    pub node: Option<AudioNode>,
    pub capture: Option<D::Capture>,
    pub stt: Option<Link>,
    pub cur: Vec<i16>,
    pub say: Say,
    pub player: Player,
    pub played: mpsc::UnboundedReceiver<Played>,
    pub stt_ready: Arc<Mutex<Readiness>>,
    pub tts_ready: Readiness,
}

async fn capture_next<C: CaptureStream>(capture: &mut Option<C>) -> Option<Vec<i16>> {
    match capture {
        Some(stream) => stream.next().await,
        None => pending().await,
    }
}

async fn link_next(link: &mut Option<Link>) -> Option<LinkEvent> {
    match link {
        Some(link) => link.rx.recv().await,
        None => pending().await,
    }
}

impl<D: AudioDevice + 'static, T: Transport + 'static, W: Warm, U: UseSource> Core<D, T, W, U>
where
    T::Session: 'static,
{
    /// Runs until the inbox closes.
    pub async fn run(mut self, mut inbox: mpsc::Receiver<Command>) {
        loop {
            tokio::select! {
                command = inbox.recv() => match command {
                    Some(command) => self.command(command).await,
                    None => return,
                },
                frame = capture_next(&mut self.capture) => self.audio(frame).await,
                event = link_next(&mut self.stt) => self.stt_event(event).await,
                event = link_next(&mut self.say.link) => self.tts_event(event).await,
                Some(played) = self.played.recv() => self.played(played).await,
            }
        }
    }

    async fn command(&mut self, command: Command) {
        match command {
            Command::Begin {
                caller,
                begin,
                reply,
            } => {
                let _ = reply.send(self.begin(&caller, begin).await);
            }
            Command::Speak {
                caller,
                wire,
                reply,
            } => {
                let _ = reply.send(self.speak(&caller, wire).await);
            }
            Command::Hush { caller, reply } => {
                let _ = reply.send(self.hush(&caller).await);
            }
            Command::Prepare { caller, reply } => self.prepare(&caller, reply),
            Command::Status { reply } => {
                let _ = reply.send(self.status_text());
            }
            Command::Route {
                n,
                caller,
                target,
                reply,
            } => {
                let _ = reply.send(self.route(n, &caller, target).await);
            }
            Command::Attach { n, caller, reply } => {
                let _ = reply.send(self.attach(n, &caller).await);
            }
            Command::Release { n, caller, reply } => {
                let _ = reply.send(self.release(n, &caller).await);
            }
            Command::Cancel { n, caller, reply } => {
                let _ = reply.send(self.cancel(n, &caller).await);
            }
            Command::StopSpeech { n, caller, reply } => {
                let _ = reply.send(self.stop_speech(n, &caller).await);
            }
        }
    }

    /// The utterance `n` if it is the one held and `caller` began it.
    pub fn owned_utt(&self, n: u64, caller: &Caller) -> Result<&Utt, VoiceError> {
        match &self.utt {
            Some(utt) if utt.n == n && utt.caller == caller.unique => Ok(utt),
            _ => Err(voice_wire::VoiceRefusal::NotAllowed.into()),
        }
    }
}
