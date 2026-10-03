//! The computer-use gate: `cuad` asks leave for every pixel step and asks the person for
//! standing consent to use an app. One decision point: the same halt, the same budget, the
//! same Cedar policy and the same breaker.

use crate::deadline::within;
use crate::gate::{GateInputs, Pending, gate};
use crate::labels::absorb;
use crate::router::Router;
use crate::seams::{Clock, EventSink, GrantStore, Seams};
use crate::watch::Watch;
use action_review::RepeatState;
use docket_core::{
    ActionGrantKey, Anchor, AskReason, AuditRecord, CallProgress, CallRefusal, ConfirmAnswer,
    ConfirmAnswerKind, ConfirmDetail, ConfirmEnd, ConfirmId, ConfirmOffer, ConfirmRequest,
    Confirmer, Cost, CuaAsk, Depth, GateAnswer, Gesture, GrantAnswer, GrantAsk, GrantCaller,
    GrantTarget, Impact, IntentsReply, LabelText, Reviewed, Saw, SessionSaw, SinkIntegrity,
    TaintNote,
};
use docket_core::{AgentReach, Lasting, Millis, Origin};
use policy_point::{
    ActionFacts, CoverageState, GrantState, Op, PolicyContext, PolicyRequest, PrincipalFacts,
    SpaceRelation, TerminalGrant,
};
use porter_core::consent::{Decision, Grant, GrantScope, Usage, Verdict};
use porter_core::{Count, DataClass, GrantId};
use prov::{
    ActionName, Actor, AgentRole, Confidentiality, Effect, Integrity, Label, Source, SpaceScope,
};
use std::collections::BTreeSet;

/// How long a watching requester has to say `Proceed` before the step is refused unasked.
const PROCEED_WITHIN: Millis = Millis(10_000);

fn words(text: &str) -> LabelText {
    LabelText::parse(text).expect("fixed sheet words are valid label text")
}

impl<S: Seams> Router<S> {
    async fn ask_person(&self, request: ConfirmRequest) -> ConfirmAnswer {
        let id = request.id.clone();
        self.locked()
            .pending
            .insert(id.clone(), request.space.clone());
        let answer = self
            .seams
            .confirmer()
            .confirm(request)
            .await
            .without_terminal_grant();
        self.locked().pending.remove(&id);
        let (kind, input) = match &answer {
            ConfirmAnswer::Allowed { scope, receipt } => {
                (ConfirmAnswerKind::Allowed(*scope), Some(receipt.input))
            }
            ConfirmAnswer::AllowedFromTerminal { receipt } => {
                (ConfirmAnswerKind::AllowedFromTerminal, Some(receipt.input))
            }
            ConfirmAnswer::Ended(end) => (ConfirmAnswerKind::Ended(*end), None),
        };
        self.seams.sink().append(AuditRecord::Confirm {
            at: self.seams.clock().now(),
            id,
            answer: kind,
            input,
        });
        answer
    }

    fn sheet(
        &self,
        ask_app: &porter_core::AppName,
        space: &prov::SpaceId,
        what: &str,
        effect: Effect,
    ) -> Option<ConfirmRequest> {
        let n = self.locked().mint();
        let id = ConfirmId::parse(&format!("c-{n}")).ok()?;
        Some(ConfirmRequest {
            id,
            space: space.clone(),
            actor: Actor::System {
                part: prov::SystemPart::Cua,
            },
            app: ask_app.clone(),
            action: words(what),
            effect,
            count: Count(1),
            detail: ConfirmDetail::Plain,
            lines: vec![],
            why: vec![AskReason::FirstUse],
            taint: TaintNote::Clean,
            offer: ConfirmOffer::OnceOnly,
            gesture: Gesture::Press,
            anchor: Anchor::Centre,
            expires: self.config.confirm_expiry,
        })
    }

    /// `.Gate.Grant`: the person decides whether a run may use an app in a Space, once per
    /// data class the app declares.
    pub(crate) async fn gate_grant(&self, ask: GrantAsk) -> IntentsReply {
        let Some(request) = self.sheet(
            &ask.app,
            &ask.space,
            "Let it use this app",
            Effect::UndoableWrite,
        ) else {
            return IntentsReply::Granted(GrantAnswer::Refused(ConfirmEnd::Cancelled));
        };
        match self.ask_person(request).await {
            ConfirmAnswer::Ended(end) => IntentsReply::Granted(GrantAnswer::Refused(end)),
            ConfirmAnswer::Allowed { .. } | ConfirmAnswer::AllowedFromTerminal { .. } => {
                let now = self.seams.clock().now();
                let classes: BTreeSet<DataClass> = self
                    .locked()
                    .registry
                    .get(&ask.app)
                    .map(|m| {
                        m.manifest()
                            .actions
                            .iter()
                            .flat_map(|a| a.classes.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                for class in classes {
                    let n = self.locked().mint();
                    if let Ok(id) = GrantId::parse(&format!("g-{n}")) {
                        self.seams.grants().record(Grant {
                            id,
                            key: ActionGrantKey {
                                caller: GrantCaller::Cua,
                                owner: ask.app.clone(),
                                target: GrantTarget::App,
                                class,
                                usage: Usage::Interactive,
                                space: SpaceScope::Only(ask.space.clone()),
                            },
                            decision: Decision::Allow,
                            scope: GrantScope::Always,
                            at: now,
                        });
                    }
                }
                IntentsReply::Granted(GrantAnswer::Granted)
            }
        }
    }

    /// `.Gate.Check`: may this pixel step run? A step that Cedar leaves to a reviewer is asked
    /// of the person instead: a reviewer for pixel steps arrives with the run host.
    ///
    /// A step the person must answer is announced to a watching requester
    /// (`Progress(Confirming(id))`) before the sheet is drawn, and the router waits for its
    /// `Proceed` (the lease is suspended, so the run's own input is idle while the person
    /// answers): a requester that never proceeds, or withdraws, gets no sheet.
    pub(crate) async fn gate_check(&self, ask: CuaAsk, watch: &Watch) -> IntentsReply {
        let now = self.seams.clock().now();
        // Whatever cuad claims, the screen is somebody else's words: the run's session takes in
        // `Untrusted(Screen { app })`, so every report it sends later carries that label.
        let screen = Label::untrusted(
            Source::Screen {
                app: ask.app.clone(),
            },
            DataClass::Screen,
            ask.space.clone(),
        )
        .join(&ask.screen);
        {
            let mut st = self.locked();
            if let Some(record) = st.sessions.values_mut().find(|r| {
                matches!(&r.actor, Actor::Companion { role: AgentRole::Cua { run }, .. } if *run == ask.run)
            }) {
                absorb(record, &screen);
            }
        }
        let decided = {
            let st = self.locked();
            let Some(record) = st
                .sessions
                .values()
                .find(|r| matches!(&r.actor, Actor::Companion { role: AgentRole::Cua { run }, .. } if *run == ask.run))
            else {
                return IntentsReply::Gate(GateAnswer::Refused(CallRefusal::Halted(SpaceScope::Any)));
            };
            let consent = self
                .seams
                .grants()
                .grants()
                .iter()
                .filter(|g| {
                    g.key.caller == GrantCaller::Cua
                        && g.key.owner == ask.app
                        && g.key.space == SpaceScope::Only(ask.space.clone())
                })
                .map(|g| match g.decision {
                    Decision::Allow => Verdict::Granted {
                        grant: g.id.clone(),
                        scope: g.scope,
                    },
                    Decision::Deny => Verdict::Denied,
                })
                .max_by_key(|v| matches!(v, Verdict::Denied))
                .unwrap_or(Verdict::Ask);
            let strictness = st
                .strictness
                .get(&ask.space)
                .copied()
                .unwrap_or(self.config.strictness);
            let screen = screen.integrity;
            let ctx = PolicyContext {
                space: ask.space.clone(),
                target_space: SpaceRelation::Same,
                args: screen,
                planner: Integrity::Untrusted,
                confidentiality: Confidentiality::Public,
                usage: Usage::Background,
                origin: Origin::Companion,
                count: Count(1),
                mass_at: self.config.mass_at,
                grant: match &consent {
                    Verdict::Granted {
                        scope: GrantScope::Always,
                        ..
                    } => GrantState::Always,
                    Verdict::Granted { .. } => GrantState::Once,
                    Verdict::Ask => GrantState::None,
                    Verdict::Denied => GrantState::Denied,
                },
                terminal: TerminalGrant::NotGranted,
                coverage: CoverageState::Outside,
                task_ceiling: Effect::Read,
                strictness,
                saw: SessionSaw {
                    private: Saw::NotSeen,
                    untrusted: Saw::Seen,
                },
                sinks: SinkIntegrity {
                    recipient: Integrity::Trusted,
                    destination: Integrity::Trusted,
                    body: Integrity::Trusted,
                    path: Integrity::Trusted,
                },
                impact: if ask.effect >= Effect::Outbound {
                    Impact::High
                } else {
                    Impact::Low
                },
            };
            let ruling = self.pdp.decide(&PolicyRequest {
                principal: PrincipalFacts {
                    kind: record.actor.kind(),
                    caller: ask.app.clone(),
                    isolation: porter_core::Isolation::Unsandboxed,
                },
                op: Op::Perform,
                resource: ActionFacts {
                    app: ask.app.clone(),
                    action: ActionName::parse("cua.step")
                        .expect("`cua.step` is a valid action name"),
                    effect: ask.effect,
                    classes: BTreeSet::new(),
                    reach: AgentReach::Offered,
                    lasting: Lasting::No,
                },
                context: ctx.clone(),
            });
            let cost = Cost {
                effect: ask.effect,
                entities: Count(1),
                depth: Depth(0),
                review: Reviewed::No,
            };
            gate(&GateInputs {
                halt: &st.kill,
                space: &ask.space,
                ledger: &record.ledger,
                budget: &self.config.budget,
                cost: &cost,
                now,
                consent: &consent,
                ruling: &ruling,
                impact: ctx.impact,
                repeat: RepeatState::Fresh,
            })
        };
        match decided {
            Pending::Run => IntentsReply::Gate(GateAnswer::Run),
            Pending::Refuse(why) => IntentsReply::Gate(GateAnswer::Refused(why)),
            Pending::NeedsReview(_) | Pending::Confirm(_) => self.confirm_step(&ask, watch).await,
        }
    }

    /// Asks the person about one step, once a watching requester has stopped its own input.
    async fn confirm_step(&self, ask: &CuaAsk, watch: &Watch) -> IntentsReply {
        let refused = |end| IntentsReply::Gate(GateAnswer::Refused(CallRefusal::Unconfirmed(end)));
        let Some(request) = self.sheet(&ask.app, &ask.space, "Let it do this step", ask.effect)
        else {
            return IntentsReply::Gate(GateAnswer::Refused(CallRefusal::Timeout));
        };
        let id = request.id.clone();
        watch.tell(CallProgress::Confirming(id.clone())).await;
        let ready = within(
            self.seams.clock().after(PROCEED_WITHIN),
            within(watch.withdrawn(), watch.proceeded()),
        )
        .await;
        match ready {
            None => return refused(ConfirmEnd::Expired),
            Some(None) => return refused(ConfirmEnd::Cancelled),
            Some(Some(())) => {}
        }
        let Some(answer) = within(watch.withdrawn(), self.ask_person(request)).await else {
            self.seams.confirmer().cancel(&id).await;
            self.locked().pending.remove(&id);
            return refused(ConfirmEnd::Cancelled);
        };
        match answer {
            ConfirmAnswer::Allowed { .. } | ConfirmAnswer::AllowedFromTerminal { .. } => {
                IntentsReply::Gate(GateAnswer::Run)
            }
            ConfirmAnswer::Ended(end) => refused(end),
        }
    }
}
