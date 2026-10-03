//! Playing the scripted planner: each call or message goes through `Router::handle` with the
//! provenance its `ArgFrom` says. Values the planner could not have invented (a mail's words, a
//! contact) are held by the router as handles with their real labels; words the planner wrote
//! are passed plain and the router labels them itself.

use crate::block::block_on;
use crate::case::{ArgFrom, Case, MailField, ScriptedCall, ScriptedSend, ScriptedStep};
use crate::runner::StepEnding;
use crate::world::{Scene, companion, hold, mail_label, state_of};
use docket_core::{
    ActionRef, Args, CallRefusal, CallRequest, DenyCode, DraftPart, InboundPart, InboxAsk,
    IntentsReply, IntentsRequest, MessageDraft, Origin, ParamName, ParamType, Reveal, TargetValue,
    Value, WireRefusal,
};
use docket_fake::FakeSeams;
use docket_router::{HandleValue, Router};
use porter_core::AppName;
use prov::{
    AgentRef, Effect, EntityId, EntityKey, Integrity, Label, Labelled, MessageText, Source,
};
use std::collections::BTreeMap;

/// What an inbound message left the receiver's planner: words it may read, or a handle.
#[derive(Debug, Clone)]
enum Inbound {
    Plain(String),
    Held(docket_core::Handle),
}

/// How the planner's steps go, and what they learned from one another.
pub(crate) struct Player<'a> {
    router: &'a Router<FakeSeams>,
    case: &'a Case,
    scene: &'a Scene,
    inbound: BTreeMap<usize, Inbound>,
}

fn trusted_by(app: &AppName) -> Label {
    Label {
        integrity: Integrity::Trusted,
        confidentiality: prov::Confidentiality::Public,
        classes: Default::default(),
        sources: [Source::App(app.clone())].into(),
    }
}

impl<'a> Player<'a> {
    pub(crate) fn new(router: &'a Router<FakeSeams>, case: &'a Case, scene: &'a Scene) -> Self {
        Self {
            router,
            case,
            scene,
            inbound: BTreeMap::new(),
        }
    }

    /// Plays every step, in order.
    pub(crate) fn play(&mut self) -> Vec<StepEnding> {
        self.case
            .planner
            .iter()
            .enumerate()
            .map(|(index, step)| match step {
                ScriptedStep::Call(call) => self.call(call),
                ScriptedStep::Send(send) => self.send(index, send),
            })
            .collect()
    }

    fn mail_space(&self, msg: &str) -> prov::SpaceId {
        self.case
            .world
            .mail
            .iter()
            .find(|m| m.key == msg)
            .and_then(|m| m.space.clone())
            .unwrap_or_else(|| self.scene.space.clone())
    }

    /// The text an `ArgFrom` names, with the label the world gives it, if it has one the planner
    /// cannot supply: `None` means the words are the planner's own.
    fn source(&self, from: &ArgFrom) -> Option<(String, Option<Label>)> {
        let mail = |key: &str| self.case.world.mail.iter().find(|m| m.key == key);
        match from {
            ArgFrom::UserTurn { turn } => self
                .scene
                .turns
                .get(*turn as usize)
                .map(|t| (t.clone(), None)),
            ArgFrom::Contact(key) => {
                let contact = self.case.world.contacts.iter().find(|c| &c.key == key)?;
                let label = AppName::parse("org.quire.Mail")
                    .ok()
                    .map(|app| trusted_by(&app));
                Some((contact.address.clone(), label))
            }
            ArgFrom::MailBody(msg) => {
                mail(msg).map(|m| (m.body.clone(), Some(mail_label(self.mail_space(msg)))))
            }
            ArgFrom::MailHeader { msg, field } => mail(msg).map(|m| {
                let text = match field {
                    MailField::From => m.from.clone(),
                    MailField::Subject => m.subject.clone(),
                };
                (text, Some(mail_label(self.mail_space(msg))))
            }),
            ArgFrom::Literal(t) => Some((t.clone(), None)),
            ArgFrom::Inbound { step } => match self.inbound.get(&(*step as usize))? {
                Inbound::Plain(t) => Some((t.clone(), None)),
                Inbound::Held(h) => {
                    let st = state_of(self.router);
                    let table = &st.sessions.get(&self.scene.front)?.handles;
                    Some((table.display(*h)?.to_owned(), table.label(*h).cloned()))
                }
            },
        }
    }

    fn param_value(
        &self,
        ty: &ParamType,
        app: &AppName,
        text: String,
    ) -> Option<(Value, HandleValue)> {
        match ty {
            ParamType::Text { .. } => Some((Value::Text(text.clone()), HandleValue::Text(text))),
            ParamType::Url => Some((Value::Url(text.clone()), HandleValue::Text(text))),
            ParamType::File => {
                let file = docket_core::FileRef::parse(&text).ok()?;
                Some((Value::File(file.clone()), HandleValue::File(file)))
            }
            ParamType::Entity(kind) => {
                let id = EntityId {
                    app: app.clone(),
                    kind: kind.clone(),
                    key: EntityKey::parse(&text).ok()?,
                };
                Some((Value::Entity(id.clone()), HandleValue::Entity(id)))
            }
            _ => None,
        }
    }

    /// One argument as the planner passes it.
    fn argument(&self, call: &ScriptedCall, param: &ParamName, from: &ArgFrom) -> Option<Value> {
        let registry = state_of(self.router).registry.clone();
        let action = ActionRef {
            app: call.app.clone(),
            name: call.action.clone(),
        };
        let ty = registry
            .action(&action)?
            .params
            .iter()
            .find(|p| &p.name == param)?
            .ty
            .clone();
        let (text, label) = self.source(from)?;
        let (plain, held) = self.param_value(&ty, &call.app, text)?;
        match label {
            Some(label) => {
                let source = label.sources.iter().next().cloned().unwrap_or(Source::User);
                hold(self.router, &self.scene.front, held, label, source).map(Value::Handle)
            }
            None => {
                // The person's own words that name a thing: the person chose it.
                if let (ArgFrom::UserTurn { .. }, Value::Entity(e)) = (from, &plain)
                    && let Some(r) = state_of(self.router).sessions.get_mut(&self.scene.front)
                {
                    r.known.insert(e.clone());
                }
                Some(plain)
            }
        }
    }

    fn call(&mut self, call: &ScriptedCall) -> StepEnding {
        let args: Args = call
            .args
            .iter()
            .filter_map(|(param, from)| {
                self.argument(call, param, from).map(|v| {
                    (
                        param.clone(),
                        // A hijacked planner claims everything is trusted; the router derives
                        // its own labels and ignores the claim.
                        Labelled {
                            value: v,
                            label: Label::trusted_user(),
                        },
                    )
                })
            })
            .collect();
        let target = if call.targets.is_empty() {
            TargetValue::Nothing
        } else {
            TargetValue::Entities(
                call.targets
                    .iter()
                    .map(|t| EntityId {
                        app: call.app.clone(),
                        kind: t.kind.clone(),
                        key: t.key.clone(),
                    })
                    .collect(),
            )
        };
        let action = ActionRef {
            app: call.app.clone(),
            name: call.action.clone(),
        };
        let effect = state_of(self.router)
            .registry
            .action(&action)
            .map_or(Effect::Read, |d| d.effect);
        let asked_before = self.router.seams.confirmer.requests().len();
        let Ok(caller) = companion() else {
            return StepEnding::Refused(CallRefusal::Denied(DenyCode::NotAllowed));
        };
        let reply = block_on(self.router.handle(
            &caller,
            IntentsRequest::Perform {
                call: CallRequest {
                    action,
                    target,
                    args,
                    origin: Origin::Companion,
                },
                session: Some(self.scene.front.clone()),
                parent_window: None,
            },
        ));
        let asked = self.router.seams.confirmer.requests().len() > asked_before;
        match reply {
            IntentsReply::Performed(result) => match (*result, asked) {
                (_, true) => StepEnding::Asked(effect),
                (Ok(_), false) => StepEnding::Ran(effect),
                (Err(why), false) => StepEnding::Refused(why),
            },
            IntentsReply::Refused(WireRefusal::Call(why)) => StepEnding::Refused(why),
            _ => StepEnding::Refused(CallRefusal::Denied(DenyCode::NotAllowed)),
        }
    }

    fn send(&mut self, index: usize, send: &ScriptedSend) -> StepEnding {
        let sender = if send.from.agent == AgentRef::Companion {
            Some(&self.scene.front)
        } else {
            self.scene.workers.get(&send.from.agent)
        };
        let (Some(session), Ok(caller)) = (sender, companion()) else {
            return StepEnding::Refused(CallRefusal::Denied(DenyCode::NotAllowed));
        };
        let part = match self.source(&send.text) {
            Some((text, Some(label))) => {
                let from = label.sources.iter().next().cloned().unwrap_or(Source::User);
                match hold(self.router, session, HandleValue::Text(text), label, from) {
                    Some(h) => DraftPart::Handle(h),
                    None => return StepEnding::Refused(CallRefusal::Denied(DenyCode::NotAllowed)),
                }
            }
            Some((text, None)) => DraftPart::Text(MessageText::new(text)),
            None => DraftPart::Text(MessageText::new(String::new())),
        };
        let reply = block_on(self.router.handle(
            &caller,
            IntentsRequest::MessageSend {
                session: session.clone(),
                draft: MessageDraft {
                    to: send.to.clone(),
                    thread: None,
                    in_reply_to: None,
                    kind: send.kind,
                    parts: vec![part],
                },
            },
        ));
        if !matches!(reply, IntentsReply::Delivered(_)) {
            return StepEnding::Refused(CallRefusal::Denied(DenyCode::NotAllowed));
        }
        let integrity = self
            .router
            .seams
            .sink
            .records()
            .into_iter()
            .rev()
            .find_map(|r| match r {
                docket_core::AuditRecord::Message(m) => Some(m.label.integrity),
                _ => None,
            })
            .unwrap_or(Integrity::Untrusted);
        self.take_inbox(index, &send.to.agent, &caller);
        StepEnding::Delivered(integrity)
    }

    /// The receiver's planner reads what landed: the first words of it become what a later step
    /// can name as `Inbound`.
    fn take_inbox(&mut self, index: usize, agent: &AgentRef, caller: &docket_core::CallerId) {
        if !matches!(agent, AgentRef::Companion | AgentRef::Worker { .. }) {
            return;
        }
        let reply = block_on(self.router.handle(
            caller,
            IntentsRequest::MessageInbox(InboxAsk {
                agent: agent.clone(),
                after: None,
            }),
        ));
        let IntentsReply::Inbox(lines) = reply else {
            return;
        };
        let words = lines.into_iter().next().and_then(|l| {
            l.parts.into_iter().find_map(|p| match p {
                InboundPart::Text(Reveal::Plain(t)) => Some(Inbound::Plain(t)),
                InboundPart::Text(Reveal::Handle(h)) => Some(Inbound::Held(h)),
                _ => None,
            })
        });
        if let Some(words) = words {
            self.inbound.insert(index, words);
        }
    }
}
