//! A planner model that says what a test scripted, and remembers every request it was asked.

use porter_client::{Transport, TransportError};
use porter_core::consent::Usage;
use porter_core::{
    AccountId, AccountsReply, AccountsRequest, DataClass, Locality, ModelId, Need, Tier, Tokens,
};
use porter_infer::{
    ChatReply, ChatRequest, ClientFrame, InferEvent, InferRefusal, InferReply, InferRequest,
    InferSession, JsonText, OpenOptions, ServedBy, SessionError, StopReason, TokenUsage,
    ToolCallId, ToolCallPart, ToolName,
};
use serde_json::Value as Json;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// One thing the scripted model does when asked.
#[derive(Debug, Clone)]
pub enum Say {
    /// Words and tool calls (`name`, `args`).
    Reply(String, Vec<(String, Json)>),
    /// Words cut off by the output limit.
    Cut(String),
    /// Never finishes: only being dropped ends it.
    Hang,
    /// These events first (a `Why`, a `Stage`, a `Declined`), then the inner reply.
    Routed(Vec<InferEvent>, Box<Say>),
    /// Ends in a refusal, which a `Declined` event just before it explains.
    Refuse(InferRefusal),
    /// The inner reply, after the model has been waited on once: a pull of the planner's answer
    /// is pending on its first poll.
    Late(Box<Say>),
}

/// Words only.
pub fn words(text: &str) -> Say {
    Say::Reply(text.into(), vec![])
}

/// One tool call.
pub fn call(name: &str, args: Json) -> Say {
    Say::Reply(String::new(), vec![(name.into(), args)])
}

#[derive(Debug, Default)]
struct Inner {
    script: VecDeque<Say>,
    asked: Vec<ChatRequest>,
}

/// The transport: cloned into the planner, kept by the test.
#[derive(Debug, Clone, Default)]
pub struct ScriptedInfer(Arc<Mutex<Inner>>);

impl ScriptedInfer {
    pub fn new(script: Vec<Say>) -> Self {
        let this = Self::default();
        this.push(script);
        this
    }

    pub fn push(&self, script: Vec<Say>) {
        self.0.lock().expect("lock").script.extend(script);
    }

    /// Every chat request asked so far, oldest first.
    pub fn asked(&self) -> Vec<ChatRequest> {
        self.0.lock().expect("lock").asked.clone()
    }

    /// The text of the user message of the n-th request.
    pub fn user_text(&self, n: usize) -> String {
        text_of(&self.asked()[n], 1)
    }

    /// The text of the system message of the n-th request.
    pub fn system_text(&self, n: usize) -> String {
        text_of(&self.asked()[n], 0)
    }

    /// How many scripted replies are left.
    pub fn remaining(&self) -> usize {
        self.0.lock().expect("lock").script.len()
    }
}

fn text_of(request: &ChatRequest, at: usize) -> String {
    request.messages[at]
        .parts
        .iter()
        .filter_map(|p| match p {
            porter_infer::MessagePart::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

pub struct ScriptedSession {
    inner: ScriptedInfer,
    ready: VecDeque<InferEvent>,
    hang: bool,
    late: bool,
}

/// Pending once, then ready.
struct Once(bool);

impl std::future::Future for Once {
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<()> {
        if self.0 {
            return std::task::Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        std::task::Poll::Pending
    }
}

pub fn served() -> ServedBy {
    ServedBy {
        account: AccountId::parse("local").expect("account"),
        model: ModelId::parse("scripted").expect("model"),
        locality: Locality::OnDevice,
    }
}

fn reply(text: String, calls: Vec<(String, Json)>) -> InferEvent {
    let tool_calls: Vec<ToolCallPart> = calls
        .into_iter()
        .enumerate()
        .map(|(i, (name, args))| ToolCallPart {
            id: ToolCallId(format!("c{i}")),
            name: ToolName::parse(&name).expect("tool name"),
            args: JsonText::parse(&args.to_string()).expect("json"),
        })
        .collect();
    let stop = if tool_calls.is_empty() {
        StopReason::EndTurn
    } else {
        StopReason::ToolUse
    };
    InferEvent::Finished(InferReply::Chat(ChatReply {
        text,
        tool_calls,
        stop,
        thought: None,
        usage: TokenUsage {
            input: Tokens(0),
            output: Tokens(0),
            cached: Tokens(0),
        },
        served: served(),
    }))
}

impl InferSession for ScriptedSession {
    async fn send(&mut self, frame: ClientFrame) -> Result<(), SessionError> {
        let ClientFrame::Request(InferRequest::Chat(request)) = frame else {
            return Err(SessionError::Malformed("not a chat".into()));
        };
        let mut inner = self.inner.0.lock().expect("lock");
        inner.asked.push(request);
        let mut next = inner.script.pop_front();
        loop {
            match next {
                Some(Say::Routed(events, then)) => {
                    self.ready.extend(events);
                    next = Some(*then);
                }
                Some(Say::Late(then)) => {
                    self.late = true;
                    next = Some(*then);
                }
                _ => break,
            }
        }
        match next {
            Some(Say::Reply(text, calls)) => self.ready.push_back(reply(text, calls)),
            Some(Say::Cut(text)) => {
                let mut cut = reply(text, vec![]);
                if let InferEvent::Finished(InferReply::Chat(chat)) = &mut cut {
                    chat.stop = StopReason::MaxTokens;
                }
                self.ready.push_back(cut);
            }
            Some(Say::Refuse(why)) => self
                .ready
                .push_back(InferEvent::Finished(InferReply::Refused(why))),
            Some(Say::Hang) => self.hang = true,
            Some(Say::Routed(..) | Say::Late(_)) | None => return Err(SessionError::Closed),
        }
        Ok(())
    }

    async fn next(&mut self) -> Result<InferEvent, SessionError> {
        if self.hang {
            std::future::pending::<()>().await;
        }
        if std::mem::take(&mut self.late) {
            Once(false).await;
        }
        self.ready.pop_front().ok_or(SessionError::Closed)
    }
}

impl Transport for ScriptedInfer {
    type Session = ScriptedSession;

    async fn call(&self, _request: AccountsRequest) -> Result<AccountsReply, TransportError> {
        Err(TransportError::Unreachable)
    }

    async fn open_with(
        &self,
        _need: &Need,
        _class: DataClass,
        _tier: Tier,
        _options: &OpenOptions,
    ) -> Result<ScriptedSession, TransportError> {
        Ok(ScriptedSession {
            inner: self.clone(),
            ready: VecDeque::new(),
            hang: false,
            late: false,
        })
    }
}

/// Whether a request was background work.
pub fn is_background(request: &ChatRequest) -> bool {
    request.usage == Usage::Background
}
