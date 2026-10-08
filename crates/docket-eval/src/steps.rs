//! Playing the scripted planner: each call or message goes through `Router::handle` with the
//! provenance its `ArgFrom` says. Values the planner could not have invented (a mail's words, a
//! contact) are held by the router as handles with their real labels; words the planner wrote
//! are passed plain and the router labels them itself.

use crate::block::block_on;
use crate::case::{ArgFrom, Case, Driver, MailField, ScriptedCall, ScriptedSend, ScriptedStep};
use crate::runner::{Rig, StepEnding};
use crate::world::{Scene, cli, companion, hold, mail_label, state_of};
use docket_core::{
    ActionRef, Args, CallRefusal, CallRequest, DenyCode, DraftPart, InboundPart, InboxAsk,
    IntentsReply, IntentsRequest, MessageDraft, Origin, ParamDecl, ParamNeed, ParamType, Reveal,
    TargetValue, Value, WireRefusal,
};
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
pub(crate) struct Player<'a, S: Rig> {
    router: &'a Router<S>,
    case: &'a Case,
    scene: &'a Scene,
    inbound: BTreeMap<usize, Inbound>,
}

/// The scripted step could not be played as written: the case file or the harness is at fault,
/// not the router.
struct Fault(&'static str);

impl Fault {
    fn ending(self) -> StepEnding {
        StepEnding::Harness(self.0.to_owned())
    }
}

/// A case file that names a parameter the action lacks and leaves a required one out has
/// misspelled it: a planner that invents an extra argument still gives the real ones.
fn misspelled(call: &ScriptedCall, declared: &[ParamDecl]) -> Option<Fault> {
    let invented = call
        .args
        .keys()
        .any(|name| !declared.iter().any(|d| &d.name == name));
    let left_out = declared
        .iter()
        .any(|d| d.need == ParamNeed::Required && !call.args.contains_key(&d.name));
    (invented && left_out).then_some(Fault(
        "an argument the manifest does not declare, with a required one left out",
    ))
}

fn trusted_by(app: &AppName) -> Label {
    Label {
        integrity: Integrity::Trusted,
        confidentiality: prov::Confidentiality::Public,
        classes: Default::default(),
        sources: [Source::App(app.clone())].into(),
    }
}

impl<'a, S: Rig> Player<'a, S> {
    pub(crate) fn new(router: &'a Router<S>, case: &'a Case, scene: &'a Scene) -> Self {
        Self {
            router,
            case,
            scene,
            inbound: BTreeMap::new(),
        }
    }

    /// Plays every step, in order, telling `after` how each ended as it does.
    pub(crate) fn play(&mut self, mut after: impl FnMut(&StepEnding)) -> Vec<StepEnding> {
        let mut endings = Vec::new();
        for (index, step) in self.case.expanded().iter().enumerate() {
            let ending = match step {
                ScriptedStep::Call(call) => self.call(call),
                ScriptedStep::Send(send) => self.send(index, send),
            };
            after(&ending);
            endings.push(ending);
        }
        endings
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
            ArgFrom::Unminted(_) => None,
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

    /// The parameters the manifest declares for the call's action; none if it declares no such
    /// action.
    fn declared(&self, call: &ScriptedCall) -> Vec<ParamDecl> {
        let action = ActionRef {
            app: call.app.clone(),
            name: call.action.clone(),
        };
        let st = state_of(self.router);
        st.registry
            .action(&action)
            .map(|def| def.params.clone())
            .unwrap_or_default()
    }

    /// One argument as the planner passes it, or the reason the case file names one that
    /// cannot be built: words from something the world lacks, or that do not fit the type.
    /// A parameter the manifest does not declare goes through as text: a planner may invent
    /// one, and the router is what rules on it.
    fn argument(
        &self,
        call: &ScriptedCall,
        ty: Option<&ParamType>,
        from: &ArgFrom,
    ) -> Result<Value, Fault> {
        if let ArgFrom::Unminted(n) = from {
            return Ok(Value::Handle(docket_core::Handle(*n)));
        }
        let (text, label) = self
            .source(from)
            .ok_or(Fault("an argument whose source the world lacks"))?;
        let (plain, held) = match ty {
            Some(ty) => self
                .param_value(ty, &call.app, text)
                .ok_or(Fault("an argument whose words do not fit its type"))?,
            None => (Value::Text(text.clone()), HandleValue::Text(text)),
        };
        // A terminal has no handles: whatever the words once were, they were typed.
        match label.filter(|_| self.case.driver == Driver::Companion) {
            Some(label) => {
                let source = label.sources.iter().next().cloned().unwrap_or(Source::User);
                hold(self.router, &self.scene.front, held, label, source)
                    .map(Value::Handle)
                    .ok_or(Fault("a handle the front session could not hold"))
            }
            None => {
                // The person's own words that name a thing: the person chose it.
                if self.case.driver == Driver::Companion
                    && let (ArgFrom::UserTurn { .. }, Value::Entity(e)) = (from, &plain)
                    && let Some(r) = state_of(self.router).sessions.get_mut(&self.scene.front)
                {
                    r.known.insert(e.clone());
                }
                Ok(plain)
            }
        }
    }

    fn call(&mut self, call: &ScriptedCall) -> StepEnding {
        let declared = self.declared(call);
        if let Some(fault) = misspelled(call, &declared) {
            return fault.ending();
        }
        let args: Result<Args, Fault> = call
            .args
            .iter()
            .map(|(param, from)| {
                let ty = declared.iter().find(|d| &d.name == param).map(|d| &d.ty);
                self.argument(call, ty, from).map(|v| {
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
        let args = match args {
            Ok(args) => args,
            Err(fault) => return fault.ending(),
        };
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
        let asked_before = self.router.seams.confirmer().requests().len();
        let (who, origin, session) = match self.case.driver {
            Driver::Companion => (
                companion(),
                Origin::Companion,
                Some(self.scene.front.clone()),
            ),
            Driver::Cli => (cli(), Origin::Cli, None),
        };
        let Ok(caller) = who else {
            return Fault("the caller identity did not parse").ending();
        };
        let reply = block_on(self.router.handle(
            &caller,
            IntentsRequest::Perform {
                activation: None,
                call: CallRequest {
                    action,
                    target,
                    args,
                    origin,
                },
                session,
                parent_window: None,
            },
        ));
        let asked = self.router.seams.confirmer().requests().len() > asked_before;
        match reply {
            IntentsReply::Performed(result) => match (*result, asked) {
                (_, true) => StepEnding::Asked(effect),
                (Ok(_), false) => StepEnding::Ran(effect),
                (Err(why), false) => StepEnding::Refused(why),
            },
            IntentsReply::Refused(WireRefusal::Call(why)) => StepEnding::Refused(why),
            IntentsReply::Refused(WireRefusal::NotAllowed) => {
                StepEnding::Refused(CallRefusal::Denied(DenyCode::NotAllowed))
            }
            _ => Fault("a reply of a kind a call never gets").ending(),
        }
    }

    fn send(&mut self, index: usize, send: &ScriptedSend) -> StepEnding {
        let sender = if send.from.agent == AgentRef::Companion {
            Some(&self.scene.front)
        } else {
            self.scene.workers.get(&send.from.agent)
        };
        let (Some(session), Ok(caller)) = (sender, companion()) else {
            return Fault("a sender with no session").ending();
        };
        let part = match self.source(&send.text) {
            Some((text, Some(label))) => {
                let from = label.sources.iter().next().cloned().unwrap_or(Source::User);
                match hold(self.router, session, HandleValue::Text(text), label, from) {
                    Some(h) => DraftPart::Handle(h),
                    None => return Fault("a handle the sender could not hold").ending(),
                }
            }
            Some((text, None)) => DraftPart::Text(MessageText::new(text)),
            None => return Fault("message words whose source the world lacks").ending(),
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
        match reply {
            IntentsReply::Delivered(_) => {}
            IntentsReply::Refused(WireRefusal::Send(_) | WireRefusal::NotAllowed) => {
                return StepEnding::Refused(CallRefusal::Denied(DenyCode::NotAllowed));
            }
            _ => return Fault("a reply of a kind a send never gets").ending(),
        }
        let integrity = self
            .router
            .seams
            .sink()
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
