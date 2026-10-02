//! A scripted model and the fixture request, shared by the reviewer tests.
#![allow(dead_code)]

use action_review::*;
use docket_core::*;
use porter_core::{AccountId, Billing, Locality, ModelId, Tokens};
use porter_infer::{
    ChatReply, ChatRequest, ChatSink, EmbedReply, EmbedRequest, Model, ModelCard, ModelError,
    ServedBy, StopReason, TokenUsage,
};
use prov::{ActionName, Effect, Integrity, SpaceId};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

/// Polls a future that never waits (the scripted models answer at once).
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(out) = future.as_mut().poll(&mut cx) {
            return out;
        }
    }
}

#[derive(Debug, Clone)]
pub struct Scripted {
    pub card: ModelCard,
    replies: Arc<Mutex<VecDeque<Result<ChatReply, ModelError>>>>,
    seen: Arc<Mutex<Vec<ChatRequest>>>,
}

pub fn served(card: &ModelCard) -> ServedBy {
    ServedBy {
        account: card.account.clone(),
        model: card.model.clone(),
        locality: card.locality.clone(),
    }
}

impl Scripted {
    pub fn new(name: &str, replies: Vec<Result<ChatReply, ModelError>>) -> Self {
        Self {
            card: ModelCard {
                account: AccountId::parse("local").expect("account"),
                model: ModelId::parse(name).expect("model"),
                locality: Locality::OnDevice,
                billing: Billing::Free,
                capabilities: vec![],
            },
            replies: Arc::new(Mutex::new(replies.into())),
            seen: Arc::new(Mutex::new(vec![])),
        }
    }

    pub fn saying(name: &str, text: &str) -> Self {
        let model = Self::new(name, vec![]);
        model.push(Ok(reply(&model.card, text, StopReason::EndTurn)));
        model
    }

    pub fn push(&self, reply: Result<ChatReply, ModelError>) {
        self.replies.lock().expect("lock").push_back(reply);
    }

    pub fn calls(&self) -> usize {
        self.seen.lock().expect("lock").len()
    }

    pub fn last(&self) -> ChatRequest {
        self.seen
            .lock()
            .expect("lock")
            .last()
            .cloned()
            .expect("a call")
    }
}

pub fn reply(card: &ModelCard, text: &str, stop: StopReason) -> ChatReply {
    ChatReply {
        text: text.to_owned(),
        tool_calls: vec![],
        stop,
        thought: None,
        usage: TokenUsage {
            input: Tokens(1),
            output: Tokens(1),
            cached: Tokens(0),
        },
        served: served(card),
    }
}

impl Model for Scripted {
    fn card(&self) -> &ModelCard {
        &self.card
    }

    async fn chat(
        &self,
        request: &ChatRequest,
        _sink: &mut impl ChatSink,
    ) -> Result<ChatReply, ModelError> {
        self.seen.lock().expect("lock").push(request.clone());
        self.replies
            .lock()
            .expect("lock")
            .pop_front()
            .unwrap_or(Err(ModelError::Unreachable))
    }

    async fn embed(&self, _request: &EmbedRequest) -> Result<EmbedReply, ModelError> {
        Err(ModelError::Unreachable)
    }
}

pub fn timeouts() -> ReviewTimeouts {
    ReviewTimeouts {
        quick: Millis(300),
        deliberate: Millis(3000),
        second: Millis(3000),
    }
}

pub fn reviewer(
    quick: &Scripted,
    deliberate: &Scripted,
    second: &Scripted,
) -> InferReviewer<Scripted> {
    InferReviewer {
        quick: quick.clone(),
        deliberate: deliberate.clone(),
        second: second.clone(),
        timeouts: timeouts(),
    }
}

pub fn request() -> ReviewRequest {
    ReviewRequest {
        space: SpaceId::parse("work").expect("space"),
        strictness: Strictness::Default,
        turns: vec![UserTurn {
            id: TurnId(1),
            text: "archive the newsletters".into(),
            at: prov::UnixSeconds(1),
            from: TurnSource::Launcher,
            via: TurnVia::Typed,
        }],
        proposed: ProposedAction {
            app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
            action: ActionName::parse("mail.thread.archive").expect("action"),
            label: LabelText::parse("Archive").expect("label"),
            effect: Effect::UndoableWrite,
            kinds: BTreeSet::new(),
            count: porter_core::Count(2),
            args: vec![],
            lasting: Lasting::No,
        },
        labels: ArgLabels {
            per_arg: BTreeMap::new(),
            planner: Integrity::Trusted,
            saw: SessionSaw {
                private: Saw::NotSeen,
                untrusted: Saw::NotSeen,
            },
        },
        task_policy: None,
        history: vec![],
    }
}

pub const ALLOW: &str = r#"{"verdict":"allow","code":"within_request","reason":"asked for"}"#;
pub const DENY: &str = r#"{"verdict":"deny","code":"outside_request","reason":"not asked"}"#;
pub const ASK: &str = r#"{"verdict":"ask","code":"uncertain","reason":"unsure"}"#;

pub fn silent() -> Scripted {
    Scripted::new("silent", vec![])
}
