//! The tap on a daemon's inferd link: every chat request and its answer, as the daemon saw
//! them, kept as [`ModelExchange`] lines for a live run's trace and for turning a live failure
//! into a cassette. It is the one place the words a model was sent and said can be read back:
//! inferd's own audit never keeps content, and its replay `record` sees requests only.
//!
//! THIS WRITES PROMPTS TO DISK. The environment can switch it on only in a build with the
//! `test-model-trace` feature, which only the live harness (`docket-accept`) enables: there the
//! process must also be started with `DOCKET_MODEL_TRACE=<file>` (the harness's scratch
//! directory; the file is created 0600 and appended to, one JSON line per exchange). In any
//! other build the variable is ignored, with one stderr line, so a release daemon's environment
//! cannot make it write prompts; `scripts/check-boundary.sh` checks that no daemon's default
//! build enables the feature. A daemon that is not asked takes no copy of anything. A process that is not a daemon builds a
//! [`MemoryTap`] and reads the exchanges itself.

use docket_core::{ExchangeAnswer, ExchangeCall, ExchangeMessage, ModelExchange};
use porter_client::{Relayed, Transport, TransportError};
use porter_core::{AccountsReply, AccountsRequest, DataClass, EndpointUrl, GrantId, Need, Tier};
use porter_infer::{
    ChatRequest, ClientFrame, InferEvent, InferReply, InferRequest, InferSession, MessagePart,
    OpenOptions, Readiness, ReplyShape, Role, SessionError,
};
use serde::Serialize;
use std::ffi::OsString;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

/// Whether this build may let the environment switch the tap on.
#[derive(Debug, Clone, Copy)]
enum Build {
    Harness,
    Release,
}

/// The variable that names the file the tap appends to.
pub const TRACE_VAR: &str = "DOCKET_MODEL_TRACE";

/// Exchanges kept in memory, for the process that runs the whole case itself.
#[derive(Debug, Clone, Default)]
pub struct MemoryTap(Arc<Mutex<Vec<ModelExchange>>>);

impl MemoryTap {
    /// Takes every exchange kept so far, oldest first, leaving none.
    pub fn take(&self) -> Vec<ModelExchange> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

#[derive(Debug, Clone)]
enum Sink {
    Off,
    File(Arc<PathBuf>),
    Memory(MemoryTap),
}

/// Where exchanges go, and who is asking.
#[derive(Debug, Clone)]
pub struct Tap {
    sink: Sink,
    by: Arc<str>,
    count: Arc<AtomicU32>,
}

/// The name of this process for a trace: its executable's name without the harness prefix.
fn process_label() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .map(|name| {
            name.strip_prefix("accept-")
                .map_or(name.clone(), str::to_owned)
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

impl Tap {
    fn with(sink: Sink, by: &str) -> Self {
        Self {
            sink,
            by: Arc::from(by),
            count: Arc::default(),
        }
    }

    /// A tap that keeps nothing.
    pub fn off() -> Self {
        Self::with(Sink::Off, "")
    }

    /// A tap that appends to `path`, as `by`.
    pub fn file(path: PathBuf, by: &str) -> Self {
        Self::with(Sink::File(Arc::new(path)), by)
    }

    /// A tap that keeps exchanges in `memory`, as `by`.
    pub fn memory(memory: MemoryTap, by: &str) -> Self {
        Self::with(Sink::Memory(memory), by)
    }

    /// The tap the daemon's environment asks for: a file when `DOCKET_MODEL_TRACE` names one
    /// and this build has the `test-model-trace` feature, else off.
    pub fn from_env() -> Self {
        let build = match cfg!(feature = "test-model-trace") {
            true => Build::Harness,
            false => Build::Release,
        };
        Self::chosen(std::env::var_os(TRACE_VAR), build)
    }

    fn chosen(var: Option<OsString>, build: Build) -> Self {
        match (var, build) {
            (Some(path), Build::Harness) if !path.is_empty() => {
                Self::file(PathBuf::from(path), &process_label())
            }
            (Some(path), Build::Release) if !path.is_empty() => {
                eprintln!(
                    "docket: {TRACE_VAR} is set but this build has no test-model-trace feature; ignoring it"
                );
                Self::off()
            }
            _ => Self::off(),
        }
    }

    fn is_off(&self) -> bool {
        matches!(self.sink, Sink::Off)
    }

    fn keep(&self, exchange: ModelExchange) {
        match &self.sink {
            Sink::Off => {}
            Sink::Memory(memory) => memory
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(exchange),
            Sink::File(path) => {
                if let Err(why) = append(path, &exchange) {
                    eprintln!("docket: model trace: {}: {why}", path.display());
                }
            }
        }
    }
}

fn append(path: &PathBuf, exchange: &ModelExchange) -> std::io::Result<()> {
    let mut line = serde_json::to_string(exchange).map_err(std::io::Error::other)?;
    line.push('\n');
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?
        .write_all(line.as_bytes())
}

/// A transport that tells its tap about every chat turn and otherwise is `T`.
#[derive(Debug)]
pub struct Tapped<T> {
    inner: T,
    tap: Tap,
}

impl<T> Tapped<T> {
    /// `inner`, with `tap` on its sessions.
    pub fn new(inner: T, tap: Tap) -> Self {
        Self { inner, tap }
    }

    /// The transport underneath.
    pub fn inner(&self) -> &T {
        &self.inner
    }
}

/// The slug of a closed set, from its serde form.
fn slug<V: Serialize>(value: &V) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        Ok(other) => other.to_string(),
        Err(_) => String::new(),
    }
}

/// The words of one part of a message.
fn words(part: &MessagePart) -> Option<String> {
    match part {
        MessagePart::Text(text) => Some(text.clone()),
        MessagePart::Image(_) => Some("[image]".to_owned()),
        MessagePart::ToolCall(call) => {
            Some(format!("[call {} {}]", slug(&call.name), slug(&call.args)))
        }
        MessagePart::ToolResult(result) => {
            let inner: Vec<String> = result.parts.iter().filter_map(words).collect();
            Some(format!("[result {:?}: {}]", result.status, inner.join(" ")))
        }
        MessagePart::Thought(_) => None,
    }
}

fn role(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

fn shape(shape: &ReplyShape) -> &'static str {
    match shape {
        ReplyShape::Text => "text",
        ReplyShape::Json(_) => "json",
        ReplyShape::Choice(_) => "choice",
    }
}

/// An exchange begun: everything but the answer.
fn begin(by: &str, n: u32, request: &ChatRequest) -> ModelExchange {
    ModelExchange {
        by: by.to_owned(),
        n,
        tier: slug(&request.tier),
        class: slug(&request.class),
        shape: shape(&request.shape).to_owned(),
        tools: request.tools.iter().map(|t| slug(&t.name)).collect(),
        messages: request
            .messages
            .iter()
            .map(|m| ExchangeMessage {
                role: role(m.role).to_owned(),
                text: m
                    .parts
                    .iter()
                    .filter_map(words)
                    .collect::<Vec<_>>()
                    .join("\n"),
            })
            .collect(),
        route: Vec::new(),
        answer: ExchangeAnswer::Cancelled,
        took_ms: 0,
        input_tokens: 0,
        output_tokens: 0,
    }
}

/// What inferd said about the route, in one line, if the event is such a note.
fn route_note(event: &InferEvent) -> Option<String> {
    match event {
        InferEvent::Routed(served) => Some(format!(
            "routed to {}/{} ({})",
            slug(&served.account),
            slug(&served.model),
            slug(&served.locality)
        )),
        InferEvent::Why(why) => Some(format!("why: {}", slug(why))),
        InferEvent::Declined(declined) => Some(format!("declined: {declined:?}")),
        _ => None,
    }
}

/// The exchange finished by `reply`.
fn finish(mut open: ModelExchange, reply: &InferReply, took_ms: u32) -> ModelExchange {
    open.took_ms = took_ms;
    open.answer = match reply {
        InferReply::Chat(chat) => {
            open.input_tokens = chat.usage.input.0;
            open.output_tokens = chat.usage.output.0;
            ExchangeAnswer::Replied {
                text: chat.text.clone(),
                calls: chat
                    .tool_calls
                    .iter()
                    .map(|c| ExchangeCall {
                        name: slug(&c.name),
                        args: slug(&c.args),
                    })
                    .collect(),
                stop: slug(&chat.stop),
            }
        }
        InferReply::Refused(why) => ExchangeAnswer::Refused(format!("{why:?}")),
        InferReply::Failed(why) => ExchangeAnswer::Failed(format!("{why:?}")),
        InferReply::Cancelled => ExchangeAnswer::Cancelled,
        other => ExchangeAnswer::Failed(format!("unexpected reply {other:?}")),
    };
    open
}

/// A session that tells its tap about each chat turn.
#[derive(Debug)]
pub struct TappedSession<S> {
    inner: S,
    tap: Tap,
    open: Option<(ModelExchange, Instant)>,
}

impl<S> TappedSession<S> {
    fn note_frame(&mut self, frame: &ClientFrame) {
        if self.tap.is_off() {
            return;
        }
        if let ClientFrame::Request(InferRequest::Chat(request)) = frame {
            let n = self.tap.count.fetch_add(1, Ordering::Relaxed) + 1;
            self.open = Some((begin(&self.tap.by, n, request), Instant::now()));
        }
    }

    fn note_event(&mut self, event: &InferEvent) {
        let Some((open, began)) = self.open.as_mut() else {
            return;
        };
        if let Some(note) = route_note(event) {
            open.route.push(note);
        }
        if let InferEvent::Finished(reply) = event {
            let took = u32::try_from(began.elapsed().as_millis()).unwrap_or(u32::MAX);
            if let Some((open, _)) = self.open.take() {
                self.tap.keep(finish(open, reply, took));
            }
        }
    }
}

impl<S: InferSession> InferSession for TappedSession<S> {
    async fn send(&mut self, frame: ClientFrame) -> Result<(), SessionError> {
        self.note_frame(&frame);
        self.inner.send(frame).await
    }

    async fn send_attached(
        &mut self,
        frame: ClientFrame,
        attachments: Vec<std::os::fd::OwnedFd>,
    ) -> Result<(), SessionError> {
        self.note_frame(&frame);
        self.inner.send_attached(frame, attachments).await
    }

    async fn next(&mut self) -> Result<InferEvent, SessionError> {
        let event = self.inner.next().await?;
        self.note_event(&event);
        Ok(event)
    }
}

impl<T: Transport> Transport for Tapped<T> {
    type Session = TappedSession<T::Session>;

    async fn call(&self, request: AccountsRequest) -> Result<AccountsReply, TransportError> {
        self.inner.call(request).await
    }

    async fn open_authenticated(
        &self,
        grant: &GrantId,
        endpoint: &EndpointUrl,
    ) -> Result<Relayed, TransportError> {
        self.inner.open_authenticated(grant, endpoint).await
    }

    async fn open_linked(
        &self,
        grant: &GrantId,
        origin: &EndpointUrl,
    ) -> Result<Relayed, TransportError> {
        self.inner.open_linked(grant, origin).await
    }

    async fn open_with(
        &self,
        need: &Need,
        class: DataClass,
        tier: Tier,
        options: &OpenOptions,
    ) -> Result<Self::Session, TransportError> {
        let inner = self.inner.open_with(need, class, tier, options).await?;
        Ok(TappedSession {
            inner,
            tap: self.tap.clone(),
            open: None,
        })
    }

    async fn prepare(
        &self,
        need: &Need,
        class: DataClass,
        tier: Tier,
        options: &OpenOptions,
    ) -> Result<Readiness, TransportError> {
        self.inner.prepare(need, class, tier, options).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tap_that_is_off_keeps_nothing() {
        assert!(Tap::off().is_off());
        let memory = MemoryTap::default();
        Tap::off().keep(finish(begin("t", 1, &sample()), &InferReply::Cancelled, 3));
        assert!(memory.take().is_empty());
    }

    fn sample() -> ChatRequest {
        ChatRequest {
            messages: vec![],
            shape: ReplyShape::Text,
            tier: Tier::Fast,
            class: DataClass::Prompt,
            usage: porter_core::consent::Usage::Interactive,
            tools: vec![],
            control: porter_infer::ChatControl {
                tool_choice: porter_infer::ToolChoice::Never,
                tool_calls: porter_infer::ToolParallelism::One,
                max_output: porter_infer::Knob::Off,
                reasoning: porter_infer::Reasoning::Off,
                sampling: porter_infer::Knob::Off,
                stop: vec![],
            },
        }
    }

    #[test]
    fn a_release_build_ignores_the_trace_variable() {
        let var = Some(OsString::from("/scratch/model.jsonl"));
        assert!(Tap::chosen(var, Build::Release).is_off());
    }

    #[test]
    fn a_harness_build_follows_the_trace_variable() {
        let var = Some(OsString::from("/scratch/model.jsonl"));
        assert!(!Tap::chosen(var, Build::Harness).is_off());
        assert!(Tap::chosen(Some(OsString::new()), Build::Harness).is_off());
        assert!(Tap::chosen(None, Build::Harness).is_off());
    }

    #[test]
    fn a_memory_tap_hands_each_exchange_over_once_in_order() {
        let memory = MemoryTap::default();
        let tap = Tap::memory(memory.clone(), "eval");
        for n in 1..=2 {
            tap.keep(finish(
                begin("eval", n, &sample()),
                &InferReply::Cancelled,
                1,
            ));
        }
        let taken = memory.take();
        assert_eq!(taken.iter().map(|e| e.n).collect::<Vec<_>>(), [1, 2]);
        assert!(memory.take().is_empty());
        assert_eq!(taken[0].tier, "fast");
        assert_eq!(taken[0].class, "prompt");
    }
}
