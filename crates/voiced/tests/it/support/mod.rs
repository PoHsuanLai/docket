//! The harness: voiced served on a private bus over a scripted device and a scripted inferd, with
//! a shell client, scratch HOME and XDG, and no real device, daemon or session bus.

#![allow(dead_code)]

pub mod device;
pub mod infer;

use device::{Hand, ScriptedDevice};
use docket_testbus::PrivateBus;
use infer::{ScriptedInfer, Seen};
use porter_core::{AccountId, Locality, ModelId};
use porter_fake::{Script, ScriptStep};
use porter_infer::{
    AudioFrameOut, AudioRate, Base64Bytes, HeardDelta, InferEvent, InferReply, Readiness,
    RequestKind, ServedBy, SpeakReply, TranscribeReply,
};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::io::AsyncReadExt;
use tokio::net::UnixStream;
use voice_wire::{
    HeardText, UtteranceEnd, VoiceBegin, VoiceEvent, VoiceTarget, VoiceTrigger, VoiceUse,
};
use voiced::{
    FixedUse, FixedWarm, Running, Seams, Unframed, UtteranceProxy, VoiceProxy, VoicedConfig, seal,
    unframe,
};
use zbus::zvariant::OwnedFd;

pub use docket_core::VoiceIntent;

pub const SHELL: &str = "org.quire.Shell";
pub const MAILO: &str = "org.quire.Mailo";

pub fn served() -> ServedBy {
    ServedBy {
        account: AccountId::parse("local").expect("account"),
        model: ModelId::parse("scripted-stt").expect("model"),
        locality: Locality::OnDevice,
    }
}

/// A quiet frame and a loud one, 512 samples each.
pub fn quiet() -> Vec<i16> {
    vec![0; 512]
}

pub fn loud() -> Vec<i16> {
    (0..512)
        .map(|i| if i % 2 == 0 { 9_000 } else { -9_000 })
        .collect()
}

/// The transcript script: a partial after 1024 samples, a segment after 2048, and the whole
/// text when the audio ends.
pub fn transcript(partial: &str, segment: &str, whole: &str) -> Vec<Script> {
    let heard = |delta| InferEvent::Heard(delta);
    vec![Script {
        kind: RequestKind::Transcribe,
        steps: vec![
            ScriptStep::AfterAudio {
                samples: 1024,
                event: heard(HeardDelta::Partial {
                    text: partial.to_owned(),
                    from: 0,
                }),
            },
            ScriptStep::AfterAudio {
                samples: 2048,
                event: heard(HeardDelta::Final {
                    text: segment.to_owned(),
                    from: 0,
                    to: 2048,
                }),
            },
            ScriptStep::AfterAudio {
                samples: u64::MAX,
                event: InferEvent::Finished(InferReply::Transcribed(TranscribeReply {
                    text: whole.to_owned(),
                    audio_ms: 0,
                    served: served(),
                })),
            },
        ],
    }]
}

/// A turn that ends with this reply as soon as the audio ends.
pub fn finish_with(reply: InferReply) -> Vec<Script> {
    vec![Script {
        kind: RequestKind::Transcribe,
        steps: vec![ScriptStep::AfterAudio {
            samples: u64::MAX,
            event: InferEvent::Finished(reply),
        }],
    }]
}

/// One synthesised sentence: a chunk of 24 kHz audio, then done.
pub fn sentence_script(samples: usize) -> Script {
    Script {
        kind: RequestKind::Speak,
        steps: vec![
            ScriptStep::Emit(InferEvent::Spoken(AudioFrameOut {
                rate: AudioRate(24_000),
                at: 0,
                pcm: Base64Bytes(vec![1; samples * 2]),
            })),
            ScriptStep::Emit(InferEvent::Finished(InferReply::Spoke(SpeakReply {
                audio_ms: 0,
                served: served(),
            }))),
        ],
    }
}

/// A sentence that never finishes: one chunk, then silence.
pub fn endless_script() -> Script {
    Script {
        kind: RequestKind::Speak,
        steps: vec![ScriptStep::Emit(InferEvent::Spoken(AudioFrameOut {
            rate: AudioRate(24_000),
            at: 0,
            pcm: Base64Bytes(vec![2; 960]),
        }))],
    }
}

pub struct Options {
    pub usage: VoiceUse,
    pub device: (ScriptedDevice, Hand),
    pub infer: ScriptedInfer,
    pub warm: Readiness,
}

impl Options {
    pub fn new(infer: ScriptedInfer) -> Self {
        Self {
            usage: VoiceUse::On,
            device: ScriptedDevice::with_a_microphone(),
            infer,
            warm: Readiness::Ready,
        }
    }
}

pub struct World {
    pub bus: PrivateBus,
    pub hand: Hand,
    pub seen: Arc<Seen>,
    pub running: Running,
    pub shell: zbus::Connection,
    pub home: TempDir,
    pub voice: VoiceProxy<'static>,
}

pub fn config() -> VoicedConfig {
    VoicedConfig::parse(&format!(
        "earcons = \"off\"\n[roles]\nshell = [\"{SHELL}\"]\napp = [\"{MAILO}\"]\n"
    ))
    .expect("config")
}

pub async fn world(options: Options) -> World {
    docket_testbus::hang_guard::arm();
    let home = TempDir::new().expect("scratch");
    // The daemon reads nothing of the person's: HOME and XDG point at the scratch directory.
    let bus = PrivateBus::start(home.path());
    let (device, hand) = options.device;
    let seen = options.infer.seen.clone();
    let seams = Seams {
        device,
        transport: options.infer,
        warm: FixedWarm(options.warm),
        usage: FixedUse(options.usage),
        proc_root: PathBuf::from("/nonexistent-proc"),
    };
    let daemon = bus.connect().await;
    let running = voiced::start(daemon, config(), seams)
        .await
        .expect("serves");
    let shell = named(&bus, SHELL).await;
    let voice = VoiceProxy::new(&shell).await.expect("proxy");
    World {
        bus,
        hand,
        seen,
        running,
        shell,
        home,
        voice,
    }
}

/// A client that owns `name`.
pub async fn named(bus: &PrivateBus, name: &str) -> zbus::Connection {
    let connection = bus.connect().await;
    connection.request_name(name).await.expect("name");
    connection
}

pub fn begin(intent: VoiceIntent) -> String {
    seal(&VoiceBegin {
        intent,
        trigger: VoiceTrigger::HoldKey,
        space: porter_core::SpaceId::parse("main").expect("space"),
    })
    .expect("seal")
}

/// The event fd of one client.
pub struct Events {
    stream: UnixStream,
    buffer: Vec<u8>,
}

impl Events {
    pub fn new(fd: OwnedFd) -> Self {
        let std_fd: std::os::fd::OwnedFd = fd.into();
        let stream = std::os::unix::net::UnixStream::from(std_fd);
        stream.set_nonblocking(true).expect("nonblocking");
        Self {
            stream: UnixStream::from_std(stream).expect("stream"),
            buffer: Vec::new(),
        }
    }

    /// The next event, or `None` at the end of the stream.
    pub async fn next(&mut self) -> Option<VoiceEvent> {
        loop {
            match unframe::<VoiceEvent>(&self.buffer) {
                Unframed::Frame { body, used } => {
                    self.buffer.drain(..used);
                    return Some(body);
                }
                Unframed::Malformed { used } => {
                    self.buffer.drain(..used);
                    continue;
                }
                Unframed::TooLong { .. } => return None,
                Unframed::Partial => {}
            }
            let mut chunk = [0_u8; 4096];
            // The timeout is only a failsafe against a hung test; nothing waits on it.
            let read = tokio::time::timeout(
                std::time::Duration::from_secs(60),
                self.stream.read(&mut chunk),
            )
            .await
            .expect("an event within a minute");
            match read {
                Ok(0) | Err(_) => return None,
                Ok(n) => self.buffer.extend_from_slice(&chunk[..n]),
            }
        }
    }

    /// Reads until `wanted` says an event is the one, returning every event up to and including
    /// it.
    pub async fn until(&mut self, wanted: impl Fn(&VoiceEvent) -> bool) -> Vec<VoiceEvent> {
        let mut seen = Vec::new();
        while let Some(event) = self.next().await {
            let done = wanted(&event);
            seen.push(event);
            if done {
                return seen;
            }
        }
        panic!("the stream ended before the event came: {seen:?}");
    }

    pub async fn until_ended(&mut self) -> Vec<VoiceEvent> {
        self.until(|e| matches!(e, VoiceEvent::Ended(_))).await
    }

    /// Everything up to the end of the stream.
    pub async fn rest(&mut self) -> Vec<VoiceEvent> {
        let mut all = Vec::new();
        while let Some(event) = self.next().await {
            all.push(event);
        }
        all
    }
}

pub fn ended_of(events: &[VoiceEvent]) -> UtteranceEnd {
    events
        .iter()
        .find_map(|e| match e {
            VoiceEvent::Ended(end) => Some(end.clone()),
            _ => None,
        })
        .expect("an Ended event")
}

pub fn heard(text: &str) -> HeardText {
    HeardText(text.to_owned())
}

/// Starts an utterance and returns its object and events.
pub async fn start_utterance(
    world: &World,
    intent: VoiceIntent,
) -> (UtteranceProxy<'static>, Events) {
    let (path, fd) = world.voice.begin(&begin(intent)).await.expect("begin");
    let utterance = UtteranceProxy::builder(&world.shell)
        .path(path)
        .expect("path")
        .build()
        .await
        .expect("utterance");
    (utterance, Events::new(fd))
}

/// Feeds `n` loud frames and waits until the daemon has seen them all (a `Level` for each).
pub async fn feed_loud(world: &World, events: &mut Events, n: usize) -> Vec<VoiceEvent> {
    for _ in 0..n {
        assert_eq!(world.hand.feed(loud()), 1, "a capture is open");
    }
    let mut seen = Vec::new();
    let mut levels = 0;
    while levels < n {
        let event = events.next().await.expect("the stream is open");
        levels += usize::from(matches!(event, VoiceEvent::Level(_)));
        seen.push(event);
    }
    seen
}

/// The frames that make the 250 ms tail (4000 samples), fed quiet.
pub fn tail_frames() -> usize {
    4_000_usize.div_ceil(512)
}

/// Yields until `condition` holds (a state another task changes), failing after many turns.
pub async fn eventually(condition: impl Fn() -> bool) {
    for _ in 0..200_000 {
        if condition() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("the condition never held");
}

fn signal_rule(interface: &'static str, member: &'static str) -> zbus::MatchRule<'static> {
    zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(interface)
        .expect("interface")
        .member(member)
        .expect("member")
        .build()
}

/// Every `Utterance.Ended` this connection is sent, subscribed before anything can emit it.
pub async fn ended_stream(connection: &zbus::Connection) -> zbus::MessageStream {
    zbus::MessageStream::for_match_rule(
        signal_rule("org.quire.Voice1.Utterance", "Ended"),
        connection,
        None,
    )
    .await
    .expect("match")
}

/// Every `Speech.Finished` this connection is sent.
pub async fn finished_stream(connection: &zbus::Connection) -> zbus::MessageStream {
    zbus::MessageStream::for_match_rule(
        signal_rule("org.quire.Voice1.Speech", "Finished"),
        connection,
        None,
    )
    .await
    .expect("match")
}

/// Every `Voice1.StatusChanged` (a broadcast).
pub async fn status_stream(connection: &zbus::Connection) -> zbus::MessageStream {
    zbus::MessageStream::for_match_rule(
        signal_rule("org.quire.Voice1", "StatusChanged"),
        connection,
        None,
    )
    .await
    .expect("match")
}

/// The text argument of the next signal on `stream`.
pub async fn next_text(stream: &mut zbus::MessageStream) -> String {
    use futures_util::StreamExt;
    let message = stream.next().await.expect("a signal").expect("a message");
    message.body().deserialize::<String>().expect("a string")
}

pub fn end_of(text: &str) -> UtteranceEnd {
    serde_json::from_str::<voice_wire::Envelope<UtteranceEnd>>(text)
        .expect("an envelope")
        .body
}

pub fn speech_end_of(text: &str) -> voice_wire::SpeechEnd {
    serde_json::from_str::<voice_wire::Envelope<voice_wire::SpeechEnd>>(text)
        .expect("an envelope")
        .body
}

pub fn status_of(text: &str) -> voice_wire::VoiceStatus {
    serde_json::from_str::<voice_wire::Envelope<voice_wire::VoiceStatus>>(text)
        .expect("an envelope")
        .body
}

/// The refusal a failed call carries.
pub fn refusal(error: zbus::Error) -> voice_wire::VoiceRefusal {
    match error {
        zbus::Error::MethodError(name, _, _) => voiced::refusal_of_name(name.as_str())
            .unwrap_or_else(|| panic!("not a refusal: {name}")),
        other => panic!("not a method error: {other}"),
    }
}

pub fn speak_wire(text: &str, utterance: Option<&str>) -> String {
    seal(&voice_wire::SpeakWire {
        utterance: utterance.map(|u| docket_core::UtteranceId::parse(u).expect("utterance id")),
        text: voice_wire::SpokenText(text.to_owned()),
        class: porter_core::DataClass::Mail,
        lang: porter_core::capability::LanguageTag::parse("en").expect("language"),
    })
    .expect("seal")
}

pub fn route_to(app: &str, serial: u64) -> String {
    seal(&VoiceTarget::App {
        app: porter_core::AppName::parse(app).expect("app"),
        serial,
    })
    .expect("seal")
}

/// A sentence long enough that the sentencer keeps it whole.
pub const FIRST: &str = "The meeting with the design team moved to Thursday afternoon.";
pub const SECOND: &str = "Please bring the updated mock-ups and the latency numbers.";

pub fn cause_wire(cause: voice_wire::CancelCause) -> String {
    seal(&cause).expect("seal")
}
