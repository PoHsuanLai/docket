//! A scripted `org.quire.Inference1` on the bus: the model of the acceptance run. inferd has no
//! replay engine (its own tests put fake OpenAI-compatible engines behind the real daemon, which
//! needs a spawned engine process and a catalog), so this speaks the daemon's wire instead: `Open`
//! returns one end of a socketpair and the other end is served with the real frames
//! (`ClientFrame` in, `InferEvent` out), so every client's own code (porter-client's D-Bus
//! transport) runs unchanged.
//!
//! Each request is told apart by what it is, and answered from that role's script:
//! the planner (needs tools), the policy writer and the reader (both structured output, told
//! apart by their fixed instructions). Everything asked is kept for the test to read, so a test
//! can prove what a model was and was not shown.

use porter_core::wire::{FrameRead, decode_frame, encode_frame};
use porter_core::{AccountId, DataClass, Locality, ModelId, Need, Tokens};
use porter_dbus::{Details, INFERENCE_BUS, INFERENCE_PATH, NeedArg, need_from_dbus};
use porter_infer::{
    ChatReply, ChatRequest, ClientFrame, InferEvent, InferRefusal, InferReply, InferRequest,
    JsonText, MessagePart, ServedBy, StopReason, TokenUsage, ToolCallId, ToolCallPart, ToolName,
};
use serde_json::Value as Json;
use std::collections::VecDeque;
use std::os::unix::net::UnixStream as StdStream;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use zbus::fdo;
use zbus::zvariant::OwnedFd;

/// Whose request it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The companion's planner: asked with tools.
    Planner,
    /// The policy writer of intentd.
    Writer,
    /// The quarantined reader of readerd.
    Reader,
    /// Anything else (consolidation, embedding).
    Other,
}

/// What a scripted model says.
#[derive(Debug, Clone)]
pub enum Say {
    /// Tool calls: (tool name, arguments).
    Calls(Vec<(String, Json)>),
    /// Words that end the turn.
    Words(String),
}

/// A planner step: sees the request, so it can name what the previous step returned.
pub type Step = Box<dyn FnMut(&ChatRequest) -> Say + Send>;

/// A step that ignores the request.
pub fn say(said: Say) -> Step {
    Box::new(move |_| said.clone())
}

#[derive(Default)]
struct Inner {
    planner: VecDeque<Step>,
    reader: VecDeque<String>,
    writer: Option<String>,
    asked: Vec<(Role, ChatRequest)>,
    opened: Vec<(Role, DataClass)>,
}

/// The script and the log: the test holds one, the served object another.
#[derive(Clone, Default)]
pub struct Model(Arc<Mutex<Inner>>);

impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Model")
    }
}

impl Model {
    fn edit<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        f(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// What the planner does next, in order.
    pub fn planner(&self, steps: Vec<Step>) {
        self.edit(|i| i.planner.extend(steps));
    }

    /// What the reader answers next (the JSON text of a structured reply), in order.
    pub fn reader(&self, replies: &[&str]) {
        self.edit(|i| i.reader.extend(replies.iter().map(|r| (*r).to_owned())));
    }

    /// The JSON text every policy-writer request is answered with.
    pub fn writer(&self, draft: &str) {
        self.edit(|i| i.writer = Some(draft.to_owned()));
    }

    /// Every request asked, oldest first, with its role.
    pub fn asked(&self) -> Vec<(Role, ChatRequest)> {
        self.edit(|i| i.asked.clone())
    }

    /// The requests of one role.
    pub fn asked_by(&self, role: Role) -> Vec<ChatRequest> {
        self.asked()
            .into_iter()
            .filter(|(r, _)| *r == role)
            .map(|(_, q)| q)
            .collect()
    }

    /// The data class of every session opened, with the role it served.
    pub fn opened(&self) -> Vec<(Role, DataClass)> {
        self.edit(|i| i.opened.clone())
    }

    /// How many planner steps are left unused.
    pub fn planner_left(&self) -> usize {
        self.edit(|i| i.planner.len())
    }
}

/// Every text of a request, system and user, as one string.
pub fn text_of(request: &ChatRequest) -> String {
    request
        .messages
        .iter()
        .flat_map(|m| m.parts.iter())
        .filter_map(|p| match p {
            MessagePart::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn role_of(need: &Need, request: &ChatRequest) -> Role {
    let text = text_of(request);
    match need {
        Need::Llm(llm)
            if llm
                .features
                .contains(&porter_core::capability::LlmFeature::Tools) =>
        {
            Role::Planner
        }
        Need::Llm(_) if text.contains("You write a task policy") => Role::Writer,
        Need::Llm(_) if text.contains("fence") => Role::Reader,
        _ => Role::Other,
    }
}

fn served() -> ServedBy {
    ServedBy {
        account: AccountId::parse("local").expect("account"),
        model: ModelId::parse("scripted").expect("model"),
        locality: Locality::OnDevice,
    }
}

fn reply(text: String, calls: Vec<(String, Json)>) -> InferReply {
    let tool_calls: Vec<ToolCallPart> = calls
        .into_iter()
        .enumerate()
        .filter_map(|(n, (name, args))| {
            Some(ToolCallPart {
                id: ToolCallId(format!("call-{n}")),
                name: ToolName::parse(&name).ok()?,
                args: JsonText::parse(&args.to_string()).ok()?,
            })
        })
        .collect();
    let stop = if tool_calls.is_empty() {
        StopReason::EndTurn
    } else {
        StopReason::ToolUse
    };
    InferReply::Chat(ChatReply {
        text,
        tool_calls,
        stop,
        thought: None,
        usage: TokenUsage {
            input: Tokens(1),
            output: Tokens(1),
            cached: Tokens(0),
        },
        served: served(),
    })
}

impl Model {
    fn answer(&self, role: Role, request: ChatRequest) -> InferReply {
        // A planner step runs outside the lock: it reads the request, not the model.
        let step = (role == Role::Planner)
            .then(|| self.edit(|i| i.planner.pop_front()))
            .flatten();
        self.edit(|i| i.asked.push((role, request.clone())));
        match (role, step) {
            (Role::Planner, Some(mut step)) => match step(&request) {
                Say::Calls(calls) => reply(String::new(), calls),
                Say::Words(words) => reply(words, vec![]),
            },
            (Role::Planner, None) => InferReply::Refused(InferRefusal::Unsupported),
            (Role::Writer, _) => match self.edit(|i| i.writer.clone()) {
                Some(draft) => reply(draft, vec![]),
                None => InferReply::Refused(InferRefusal::Unsupported),
            },
            (Role::Reader, _) => match self.edit(|i| i.reader.pop_front()) {
                Some(text) => reply(text, vec![]),
                None => InferReply::Refused(InferRefusal::Unsupported),
            },
            (Role::Other, _) => InferReply::Refused(InferRefusal::Unsupported),
        }
    }
}

/// The served object.
struct Inference {
    model: Model,
}

#[zbus::interface(name = "org.quire.Inference1")]
impl Inference {
    async fn open(
        &self,
        need: NeedArg,
        class: String,
        _tier: String,
        _options: Details,
    ) -> fdo::Result<OwnedFd> {
        let need = need_from_dbus(need).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        let class: DataClass = serde_json::from_value(Json::String(class))
            .map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        let (ours, theirs) = StdStream::pair().map_err(|e| fdo::Error::Failed(e.to_string()))?;
        ours.set_nonblocking(true)
            .map_err(|e| fdo::Error::Failed(e.to_string()))?;
        let stream = UnixStream::from_std(ours).map_err(|e| fdo::Error::Failed(e.to_string()))?;
        tokio::spawn(session(stream, need, class, self.model.clone()));
        Ok(OwnedFd::from(std::os::fd::OwnedFd::from(theirs)))
    }

    async fn prepare(
        &self,
        _need: NeedArg,
        _class: String,
        _tier: String,
        _options: Details,
    ) -> String {
        "ready".to_owned()
    }
}

async fn session(mut stream: UnixStream, need: Need, class: DataClass, model: Model) {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        while let Ok(FrameRead::Complete(envelope, used)) = decode_frame::<ClientFrame>(&buffer) {
            buffer.drain(..used);
            let ClientFrame::Request(request) = envelope.body else {
                continue;
            };
            let events = match request {
                InferRequest::Chat(chat) => {
                    let role = role_of(&need, &chat);
                    model.edit(|i| i.opened.push((role, class)));
                    vec![
                        InferEvent::Routed(served()),
                        InferEvent::Finished(model.answer(role, chat)),
                    ]
                }
                _ => vec![InferEvent::Finished(InferReply::Refused(
                    InferRefusal::Unsupported,
                ))],
            };
            for event in events {
                let Ok(bytes) = encode_frame(&event) else {
                    return;
                };
                if stream.write_all(&bytes).await.is_err() {
                    return;
                }
            }
        }
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
        }
    }
}

/// Serves the scripted model as `org.quire.Inference1` on `connection`.
pub async fn serve(connection: &zbus::Connection) -> zbus::Result<Model> {
    let model = Model::default();
    connection
        .object_server()
        .at(
            INFERENCE_PATH,
            Inference {
                model: model.clone(),
            },
        )
        .await?;
    connection.request_name(INFERENCE_BUS).await?;
    Ok(model)
}
