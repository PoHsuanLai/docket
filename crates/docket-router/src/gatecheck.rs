//! The computer-use gate: `cuad` asks leave for every pixel step and asks the person for
//! standing consent to use an app. One decision point: the same halt, the same budget, the
//! same Cedar policy and the same breaker.

use crate::deadline::within;
use crate::facts::{budget_fact, repeat_fact, run_facts, taint_fact};
use crate::gate::{GateInputs, Pending, gate};
use crate::labels::absorb;
use crate::prepared::grant_state;
use crate::router::Router;
use crate::seams::{GrantStore, Seams};
use crate::watch::Watch;
use action_review::RepeatState;
use docket_core::EventSink;
use docket_core::{
    Anchor, AuditRecord, CallProgress, CallRefusal, ConfirmAnswer, ConfirmAnswerKind,
    ConfirmDetail, ConfirmEnd, ConfirmId, ConfirmOffer, ConfirmRequest, Confirmer, Cost, Gesture,
    Reviewed, TaintNote,
};
use docket_gate_wire::{CuaAsk, GateAnswer};
use docket_manifest::{AgentReach, Lasting};
use docket_permissions_core::{
    ActionGrantKey, AskReason, GrantCaller, GrantTarget, Impact, Level, LimitCall, Saw, SessionSaw,
    SinkIntegrity,
};
use docket_vocab::{Clock, Depth, LabelText, Origin};
use docket_wire::{GrantAnswer, GrantAsk, IntentsReply};
use policy_point::{
    ActionFacts, CoverageState, Op, PolicyContext, PolicyRequest, PrincipalFacts, RunFact,
    SpaceRelation, TerminalGrant,
};
use porter_core::consent::{Decision, Grant, GrantScope, Usage, Verdict};
use porter_core::{AppName, Count, DataClass, GrantId};
use prov::{
    ActionName, Actor, AgentRole, Confidentiality, Effect, Integrity, Label, RunId, SessionId,
    Source, SpaceScope,
};
use std::collections::BTreeSet;

/// Whether `actor` is a screen run the ask is for: the run it names, or any screen run when it
/// names none.
fn is_asked_run(actor: &Actor, named: Option<&RunId>) -> bool {
    match (actor, named) {
        (
            Actor::Companion {
                role: AgentRole::Cua { run },
                ..
            },
            Some(want),
        ) => run == want,
        (
            Actor::Companion {
                role: AgentRole::Cua { .. },
                ..
            },
            None,
        ) => true,
        _ => false,
    }
}

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
            ConfirmAnswer::Allowed { scope, receipt, .. } => (
                ConfirmAnswerKind::Allowed(scope.clone()),
                Some(receipt.input),
            ),
            ConfirmAnswer::AllowedFromTerminal { receipt } => {
                (ConfirmAnswerKind::AllowedFromTerminal, Some(receipt.input))
            }
            ConfirmAnswer::Never { receipt } => (ConfirmAnswerKind::Never, Some(receipt.input)),
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
        why: Vec<AskReason>,
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
            why,
            taint: TaintNote::Clean,
            offer: ConfirmOffer::OnceOnly,
            always: Default::default(),
            limits: Default::default(),
            gesture: Gesture::Press,
            anchor: Anchor::Centre,
            expires: self.agent_config().confirm_expiry,
            editor: None,
            helper: None,
            brief: None,
        })
    }

    /// `.Gate.Grant`: the person decides whether a run may use an app in a Space, once per
    /// data class the app declares.
    pub(crate) async fn gate_grant(&self, ask: GrantAsk) -> IntentsReply {
        // The caller's own setting and what the router holds (the person's limits, the
        // administrator's floor): the stronger wins, so a stored entry can only tighten a request.
        let held = self.limits_on(&LimitCall {
            caller: &GrantCaller::Cua,
            app: &ask.app,
            action: None,
            space: &ask.space,
            effect: Effect::UndoableWrite,
        });
        let level = ask.level.over(Level::from(&held));
        if level == Level::Never {
            return IntentsReply::Granted(GrantAnswer::Never);
        }
        // The person already let this conversation's runs use the app: a later run is not asked
        // again, unless a limit or the floor says to ask every time.
        let conversation = self.conversation_of_run(&ask);
        if Level::from(&held) == Level::AlwaysAllow
            && conversation
                .as_ref()
                .is_some_and(|c| self.conversation_allowed(c, &ask.app))
        {
            self.allow_screen_runs(&ask);
            return IntentsReply::Granted(GrantAnswer::Granted);
        }
        let Some(request) = self.sheet(
            &ask.app,
            &ask.space,
            "Let it use this app",
            Effect::UndoableWrite,
            vec![AskReason::FirstUse],
        ) else {
            return IntentsReply::Granted(GrantAnswer::Refused(ConfirmEnd::Cancelled));
        };
        match self.ask_person(request).await {
            ConfirmAnswer::Ended(end) => IntentsReply::Granted(GrantAnswer::Refused(end)),
            ConfirmAnswer::Never { .. } => {
                IntentsReply::Granted(GrantAnswer::Refused(ConfirmEnd::Refused))
            }
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
                // Only an always-allow request is remembered; "ask every run" is asked again.
                let classes = match level {
                    Level::AlwaysAllow => classes,
                    Level::AlwaysAsk | Level::Never => BTreeSet::new(),
                };
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
                self.allow_screen_runs(&ask);
                if let Some(conversation) = &conversation {
                    self.allow_conversation(conversation, &ask.app);
                }
                IntentsReply::Granted(GrantAnswer::Granted)
            }
        }
    }

    /// The conversation the run an ask names works for (the run's session's chain of parents).
    /// None for an ask that names no run, or a run the router does not hold.
    fn conversation_of_run(&self, ask: &GrantAsk) -> Option<SessionId> {
        let run = ask.run.as_ref()?;
        let st = self.locked();
        let (session, _) = st
            .sessions
            .iter()
            .find(|(_, r)| is_asked_run(&r.actor, Some(run)))?;
        st.conversation_of(session)
    }

    /// Whether the person let `conversation`'s screen runs use `app`.
    fn conversation_allowed(&self, conversation: &SessionId, app: &AppName) -> bool {
        self.locked()
            .sessions
            .get(conversation)
            .is_some_and(|r| r.conversation_apps.contains(app))
    }

    /// Remembers on `conversation` that the person let its screen runs use `app`.
    fn allow_conversation(&self, conversation: &SessionId, app: &AppName) {
        if let Some(record) = self.locked().sessions.get_mut(conversation) {
            record.conversation_apps.insert(app.clone());
        }
    }

    /// Remembers on the screen runs the person allowed to use the app, so the policy sees the
    /// steps of those runs as allowed (`run`). It ends with each run's session. The ask names the
    /// run it is for; one that names none (an older `cuad`) covers every live screen run in the
    /// Space.
    fn allow_screen_runs(&self, ask: &GrantAsk) {
        let mut st = self.locked();
        st.sessions
            .values_mut()
            .filter(|r| r.space == ask.space && is_asked_run(&r.actor, ask.run.as_ref()))
            .for_each(|r| {
                r.screen_apps.insert(ask.app.clone());
            });
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
            let Some((run_session, record)) = st
                .sessions
                .iter()
                .find(|(_, r)| matches!(&r.actor, Actor::Companion { role: AgentRole::Cua { run }, .. } if *run == ask.run))
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
                        scope: g.scope.clone(),
                    },
                    Decision::Deny => Verdict::Denied,
                })
                .max_by_key(|v| matches!(v, Verdict::Denied))
                .unwrap_or(Verdict::Ask);
            // The run takes the mode of the conversation that started it (the session its opener
            // named); a run the router cannot link starts in the start mode.
            let (mode, rows) = self.mode_in_force(&st, Some(run_session));
            // The person said yes to this run: at its start sheet, or earlier with a standing
            // grant for the app. Its steps then read the screen without a question of their own.
            let allowed = if record.screen_apps.contains(&ask.app)
                || matches!(consent, Verdict::Granted { .. })
            {
                RunFact::Allowed
            } else {
                RunFact::NotAsked
            };
            // A step always reads the screen; the policy decides what that means for a run the
            // person allowed (core `tainted-always-asks`).
            let taint = taint_fact(Saw::Seen, ask.effect);
            let screen = screen.integrity;
            let saw = SessionSaw {
                private: Saw::NotSeen,
                untrusted: Saw::Seen,
            };
            let cost = Cost {
                effect: ask.effect,
                entities: Count(1),
                depth: Depth(0),
                review: Reviewed::No,
            };
            // The same facts as an agent's call: the limits (the person's, the assistant's and the
            // floor's) cover a pixel step like any other change.
            let limited = self.limits_on(&LimitCall {
                caller: &GrantCaller::Cua,
                app: &ask.app,
                action: None,
                space: &ask.space,
                effect: ask.effect,
            });
            let budget = &self.agent_config().budget;
            let run = run_facts(
                &st.kill,
                &ask.space,
                &record.ledger,
                budget,
                &cost,
                now,
                &limited,
            );
            let ctx = PolicyContext {
                space: ask.space.clone(),
                target_space: SpaceRelation::Same,
                args: screen,
                planner: Integrity::Untrusted,
                confidentiality: Confidentiality::Public,
                usage: Usage::Background,
                origin: Origin::Companion,
                count: Count(1),
                mass_at: self.agent_config().mass_at,
                grant: grant_state(&consent),
                terminal: TerminalGrant::NotGranted,
                coverage: CoverageState::Outside,
                task_ceiling: Effect::Read,
                saw,
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
                halt: run.halt,
                budget: run.budget,
                repeat: repeat_fact(RepeatState::Fresh),
                limit: run.limit,
                taint,
                run: allowed,
                mode,
                rows,
            };
            let presets = self.presets();
            let ruled = presets.pdp().decide_checked(&PolicyRequest {
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
            crate::policylog::log_faults(&ruled, &record.task);
            let ruling = ruled.ruling;
            gate(&GateInputs {
                ruling: &ruling,
                impact: ctx.impact,
                facts: &run,
                charged: budget_fact(&record.ledger, budget, &cost, now),
            })
        };
        match decided {
            Pending::Run => IntentsReply::Gate(GateAnswer::Run),
            Pending::Refuse(why) => IntentsReply::Gate(GateAnswer::Refused(why)),
            Pending::NeedsReview(_) => {
                self.confirm_step(&ask, watch, vec![AskReason::FirstUse])
                    .await
            }
            Pending::Confirm(why) => self.confirm_step(&ask, watch, why).await,
        }
    }

    /// Asks the person about one step, once a watching requester has stopped its own input.
    async fn confirm_step(&self, ask: &CuaAsk, watch: &Watch, why: Vec<AskReason>) -> IntentsReply {
        let refused = |end| IntentsReply::Gate(GateAnswer::Refused(CallRefusal::Unconfirmed(end)));
        let Some(request) =
            self.sheet(&ask.app, &ask.space, "Let it do this step", ask.effect, why)
        else {
            return IntentsReply::Gate(GateAnswer::Refused(CallRefusal::Timeout));
        };
        let id = request.id.clone();
        watch.tell(CallProgress::Confirming(id.clone())).await;
        let ready = within(
            self.seams
                .clock()
                .after(self.agent_config().confirm_proceed),
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
            ConfirmAnswer::Never { .. } => refused(ConfirmEnd::Refused),
            ConfirmAnswer::Ended(end) => refused(end),
        }
    }
}
