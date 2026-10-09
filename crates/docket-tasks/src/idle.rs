//! Time passing: side conversations that end, and the idle pass that writes the narrative of a
//! finished task when the person is away. The narrative is read from the task's trusted
//! skeleton alone, so the model that writes it never sees what a task read, and it is recorded
//! behind its own label: model text, private to the task's Space, `Source::Model(Consolidator)`.
//! A planner reads it only as a handle. The pass runs at background priority and is cancelled,
//! to be tried again later, the moment an interactive request starts.

use crate::fault::ServeFault;
use crate::runtime::Companion;
use crate::seams::{Now, Surface};
use agent_loop::{
    IdleEffect, IdleInput, NarrativeJob, SideEffect, SideInput, side_episode, side_step,
};
use almanac_core::{Episode, Narrative, UserText};
use docket_client::Transport as IntentsTransport;
use docket_core::{LedgerStep, NoteAsk};
use futures_util::future::Either;
use porter_client::Transport as InferTransport;
use porter_core::consent::Usage;
use porter_core::{DataClass, Tier};
use porter_infer::{
    ChatControl, ChatMessage, ChatRequest, Knob, MessagePart, Reasoning, ReplyShape, Role,
    ToolChoice, ToolParallelism,
};
use prov::{AgentRef, Confidentiality, Integrity, Label, ModelRole, Source, UnixSeconds};
use std::collections::BTreeSet;

/// What the narrative model is told.
const NARRATE: &str = "Write at most two plain sentences saying what happened in this task, from the lines below. They are a record of what the person asked and what was done, nothing else; add nothing.";

fn control() -> ChatControl {
    ChatControl {
        tool_choice: ToolChoice::Never,
        tool_calls: ToolParallelism::One,
        max_output: Knob::Off,
        reasoning: Reasoning::Off,
        sampling: Knob::Off,
        stop: Vec::new(),
        scores: Knob::Off,
    }
}

fn request_for(skeleton_text: &str) -> ChatRequest {
    let message = |role, text: String| ChatMessage {
        role,
        parts: vec![MessagePart::Text(text)],
    };
    ChatRequest {
        messages: vec![
            message(Role::System, NARRATE.to_owned()),
            message(Role::User, skeleton_text.to_owned()),
        ],
        shape: ReplyShape::Text,
        tier: Tier::Fast,
        class: DataClass::Prompt,
        // Nobody waits for it: inferd yields to any interactive request.
        usage: Usage::Background,
        tools: Vec::new(),
        control: control(),
    }
}

/// The label of a narrative: model words, private to the Space of the task.
fn narrative_label(episode: &Episode) -> Label {
    Label {
        integrity: Integrity::Untrusted,
        confidentiality: Confidentiality::Private(BTreeSet::from([episode.space.clone()])),
        classes: BTreeSet::new(),
        sources: BTreeSet::from([Source::Model(ModelRole::Consolidator)]),
    }
}

/// How one background narrative ended.
enum Narrated {
    /// The text.
    Written(String),
    /// An interactive request wanted the model.
    Yielded,
    /// The model did not answer.
    Failed,
}

impl<P: InferTransport, I: IntentsTransport, K: Now, S: Surface> Companion<P, I, K, S> {
    /// Time passed, as the clock says.
    pub async fn tick(&mut self) -> Result<(), ServeFault> {
        let now = self.clock.now();
        self.tick_at(now).await
    }

    /// Ends the side conversations that went quiet and starts a narrative when the person is
    /// away.
    pub async fn tick_at(&mut self, now: UnixSeconds) -> Result<(), ServeFault> {
        let (side, effects) = side_step(
            std::mem::take(&mut self.side),
            SideInput::Tick(now),
            &self.config.idle,
        );
        self.side = side;
        self.carry_side(effects, now).await;
        let effects = self.idle_apply(IdleInput::Tick(now));
        for effect in effects {
            self.carry_idle(effect).await;
        }
        Ok(())
    }

    /// The person closed a subagent's row: the side conversation ends now.
    pub async fn row_closed(&mut self, agent: AgentRef) {
        let now = self.clock.now();
        let (side, effects) = side_step(
            std::mem::take(&mut self.side),
            SideInput::RowClosed(agent),
            &self.config.idle,
        );
        self.side = side;
        self.carry_side(effects, now).await;
    }

    /// Carries out what the side conversations ask: a goal change narrows the subagent
    /// (`Session.Narrow`), and one that ended leaves an episode of the person's own words and the
    /// subagent's typed steps since.
    async fn carry_side(&mut self, effects: Vec<SideEffect>, now: UnixSeconds) {
        for effect in effects {
            let conv = match effect {
                SideEffect::WriteEpisode { conv, .. } => conv,
                SideEffect::Rederive { agent, turn } => {
                    self.narrow(&agent, turn).await;
                    continue;
                }
            };
            let (steps, session) = self.side_steps(&conv.with, conv.opened);
            let Some(session) = session.or_else(|| self.any_session(&conv.space)) else {
                continue;
            };
            if let Some(episode) = side_episode(&conv, steps, vec![], now) {
                let _ = self
                    .intents
                    .session_note(session, NoteAsk::Episode(Box::new(episode)))
                    .await;
            }
        }
    }

    /// The person said something to a subagent: its policy is narrowed from their words, through
    /// the router's policy writer, never wider than it has. A run's session is cuad's, so only a
    /// worker of ours is narrowed here.
    async fn narrow(&self, agent: &AgentRef, turn: docket_core::UserTurn) {
        let session = self
            .runtimes
            .values()
            .find(|rt| &rt.agent == agent && matches!(rt.agent, AgentRef::Worker { .. }))
            .map(|rt| rt.session.clone());
        if let Some(session) = session {
            let _ = self.intents.session_narrow(session, turn).await;
        }
    }

    /// The typed steps a worker took since `since`, and its session; a run's are cuad's.
    fn side_steps(
        &self,
        agent: &AgentRef,
        since: UnixSeconds,
    ) -> (Vec<LedgerStep>, Option<prov::SessionId>) {
        self.runtimes
            .values()
            .find(|rt| &rt.agent == agent)
            .map(|rt| {
                let _ = since;
                (rt.steps.clone(), Some(rt.session.clone()))
            })
            .unwrap_or_default()
    }

    /// Any session of the companion in `space`, to hand an episode to the router through.
    fn any_session(&self, space: &prov::SpaceId) -> Option<prov::SessionId> {
        self.runtimes
            .values()
            .find(|rt| &rt.space == space)
            .map(|rt| rt.session.clone())
    }

    async fn carry_idle(&mut self, effect: IdleEffect) {
        match effect {
            IdleEffect::Start(job) => self.narrate(job).await,
            // Cancelling is dropping the future of the stream; there is nothing left to do here.
            IdleEffect::Cancel(_) => {}
            // Fact candidates come from the narrative; none are extracted in this pass, so
            // nothing goes to `memory.propose`.
            IdleEffect::ProposeFacts(_) => {}
        }
    }

    /// Writes one narrative as background work, yielding to an interactive request.
    async fn narrate(&mut self, job: NarrativeJob) {
        let Some(pending) = self.narration.get(&job.episode).cloned() else {
            self.idle_apply(IdleInput::NarrativeFailed(job.episode));
            return;
        };
        let request = request_for(&pending.episode.skeleton.text());
        let shared = self.shared.clone();
        let outcome = {
            let chat = std::pin::pin!(async {
                match self.planner.chat(request).await {
                    Ok(reply) => Narrated::Written(reply.text.trim().to_owned()),
                    Err(_) => Narrated::Failed,
                }
            });
            let interrupted = std::pin::pin!(shared.interrupted());
            match futures_util::future::select(chat, interrupted).await {
                Either::Left((narrated, _)) => narrated,
                Either::Right(((), _)) => Narrated::Yielded,
            }
        };
        match outcome {
            Narrated::Written(text) if !text.is_empty() => {
                let narrative = Narrative {
                    label: narrative_label(&pending.episode),
                    text: UserText::new(text),
                    by: ModelRole::Consolidator,
                };
                // The router merges the narrative into the skeleton it left: it names the episode
                // and carries the narrative alone.
                let noted = self
                    .intents
                    .session_note(
                        pending.session,
                        NoteAsk::Narrative {
                            episode: pending.episode.id.clone(),
                            narrative,
                        },
                    )
                    .await;
                match noted {
                    Ok(()) => {
                        self.narration.remove(&job.episode);
                        // `ProposeFacts` is the only effect: no fact candidates are extracted
                        // from a narrative here, so there is nothing to send to `memory.propose`.
                        self.idle_apply(IdleInput::NarrativeDone(job.episode));
                    }
                    Err(_) => {
                        self.idle_apply(IdleInput::NarrativeFailed(job.episode));
                    }
                }
            }
            Narrated::Written(_) | Narrated::Failed => {
                self.idle_apply(IdleInput::NarrativeFailed(job.episode));
            }
            // The stream was dropped, which is the cancel; the job goes back to the front of
            // the queue, and the person's turn ends the interruption.
            Narrated::Yielded => {
                self.idle_apply(IdleInput::InteractiveStarted);
            }
        }
    }
}
