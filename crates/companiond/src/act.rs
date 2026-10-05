//! A card the person pressed: `Companion1.Answer.Act`. The card's `CallRequest` goes to the
//! router as a watched `Run.Perform` in the answer's session, like any call of the task, so it is
//! gated, confirmed and audited the same way. The answer follows it: the card becomes a plan
//! step, `Running` once dispatched, `NeedsYou(Confirm)` while the sheet is up, then
//! `Done { undo }` or `Failed`. Nothing here decides what is allowed.

use crate::drive::refusal_of;
use crate::fault::ServeFault;
use crate::plan::phase_of;
use crate::runtime::Companiond;
use companion_wire::AnswerPhase;
use docket_client::{PerformEvent, PerformWatch, Transport as IntentsTransport};
use docket_core::{CallId, CallRequest, CardActionId};
use porter_client::Transport as InferTransport;
use prov::{Effect, TaskId};
use std::sync::Arc;
use tokio::sync::Mutex;

/// A card's call that has begun: the request is in flight and its events are still to be read.
pub(crate) struct Acting {
    task: TaskId,
    id: CallId,
    call: CallRequest,
    effect: Effect,
    watch: PerformWatch,
}

impl Acting {
    /// Where the request lives on the bus. The router in process has no object; its path is the
    /// one the call would have, by the task's own count.
    pub(crate) fn request_path(&self) -> String {
        self.watch
            .request()
            .map_or_else(|| docket_dbus::request_path(self.id), str::to_owned)
    }
}

/// The card named by the text `Act` was given: the id itself or its JSON string.
pub(crate) fn card_id(text: &str) -> Result<CardActionId, ServeFault> {
    serde_json::from_str::<CardActionId>(text)
        .or_else(|_| CardActionId::parse(text))
        .map_err(|_| ServeFault::NoSuchCard)
}

impl<P: InferTransport, I: IntentsTransport> Companiond<P, I> {
    /// Starts the card's call and shows it as a step. The answer must offer the card.
    pub(crate) async fn begin_act(
        &mut self,
        task: &TaskId,
        card: &CardActionId,
    ) -> Result<Acting, ServeFault> {
        let rt = self.runtimes.get(task).ok_or(ServeFault::UnknownSession)?;
        let card = rt.card(card).cloned().ok_or(ServeFault::NoSuchCard)?;
        let (session, window, id) = (rt.session.clone(), rt.window.clone(), rt.next_call);
        let watch = self
            .intents
            .perform_watched(card.call.clone(), Some(session), window)
            .await
            .map_err(ServeFault::Router)?;
        if let Some(rt) = self.runtimes.get_mut(task) {
            rt.acts.begin(
                CallId(id),
                card.call.action.clone(),
                card.label,
                card.effect,
            );
            rt.phase = AnswerPhase::Streaming;
        }
        self.publish_answer(task);
        Ok(Acting {
            task: task.clone(),
            id: CallId(id),
            call: card.call,
            effect: card.effect,
            watch,
        })
    }

    /// A draft or a proposal is the answer now: its cards may be acted on.
    pub fn propose(
        &mut self,
        task: &TaskId,
        body: companion_wire::AnswerBody,
    ) -> Result<(), ServeFault> {
        let rt = self
            .runtimes
            .get_mut(task)
            .ok_or(ServeFault::UnknownSession)?;
        rt.proposal = Some(body);
        rt.acts = crate::plan::Plan::default();
        rt.phase = AnswerPhase::Done;
        self.publish_answer(task);
        Ok(())
    }

    fn act_progress(&mut self, acting: &Acting, progress: &docket_core::CallProgress) {
        if let Some(rt) = self.runtimes.get_mut(&acting.task) {
            rt.acts.progress(acting.id, progress);
            rt.phase = phase_of(progress);
        }
        self.publish_answer(&acting.task);
    }

    fn act_end(
        &mut self,
        acting: &Acting,
        result: Result<docket_core::Outcome, docket_core::CallRefusal>,
    ) {
        if let Some(rt) = self.runtimes.get_mut(&acting.task) {
            let end = rt.record_call(&acting.call, acting.effect, result);
            rt.acts.end(acting.id, &end);
            rt.phase = AnswerPhase::Done;
        }
        self.publish_answer(&acting.task);
    }
}

/// Reads the card's request to its end, telling the answer at each step. The companion is locked
/// only to write, never while waiting on the router or the person.
pub(crate) async fn follow<P, I>(companion: Arc<Mutex<Companiond<P, I>>>, mut acting: Acting)
where
    P: InferTransport,
    I: IntentsTransport,
{
    let result = loop {
        match acting.watch.next().await {
            Ok(PerformEvent::Progress(progress)) => {
                companion.lock().await.act_progress(&acting, &progress);
            }
            Ok(PerformEvent::Done(end)) => break *end,
            Err(error) => break Err(refusal_of(error)),
        }
    };
    companion.lock().await.act_end(&acting, result);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pressed() -> (crate::task::TaskRuntime, TaskId) {
        use docket_core::{ActionRef, LabelText};
        let mut rt = crate::task::TaskRuntime::new(
            prov::SessionId::parse("s-1").expect("session"),
            prov::SpaceId::desktop(),
            prov::AgentRef::Companion,
            None,
            prov::UnixSeconds(0),
        );
        rt.proposal = Some(companion_wire::AnswerBody::Text { lines: vec![] });
        rt.acts.begin(
            CallId(0),
            ActionRef {
                app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
                name: prov::ActionName::parse("mail.message.send").expect("name"),
            },
            LabelText::parse("Send").expect("label"),
            Effect::Outbound,
        );
        (rt, TaskId::parse("t-1").expect("task"))
    }

    #[test]
    fn while_the_sheet_is_up_the_pressed_card_shows_as_a_pending_step_and_needs_you() {
        use companion_wire::{AnswerBody, NeedsYou, StepWireState};
        let (mut rt, task) = pressed();
        let sheet = docket_core::ConfirmId::parse("c-1").expect("id");
        rt.phase = AnswerPhase::NeedsYou(NeedsYou::Confirm(sheet));
        let answer = rt.answer(&task);
        assert!(matches!(answer.phase, AnswerPhase::NeedsYou(_)));
        let AnswerBody::Plan(plan) = answer.body else {
            panic!("the step, not the proposal")
        };
        assert_eq!(plan.steps[0].state, StepWireState::Pending);
    }

    #[test]
    fn a_failed_step_keeps_the_answer_done_and_shows_the_step() {
        use companion_wire::{AnswerBody, StepWireState};
        let (mut rt, task) = pressed();
        let end = rt.record_call(
            &docket_core::CallRequest {
                action: docket_core::ActionRef {
                    app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
                    name: prov::ActionName::parse("mail.message.send").expect("name"),
                },
                target: docket_core::TargetValue::Nothing,
                args: docket_core::Args::new(),
                origin: docket_core::Origin::Companion,
            },
            Effect::Outbound,
            Err(docket_core::CallRefusal::Timeout),
        );
        rt.acts.end(CallId(0), &end);
        rt.phase = AnswerPhase::Done;
        let AnswerBody::Plan(plan) = rt.answer(&task).body else {
            panic!("a failed card shows its step, not a refusal")
        };
        assert!(matches!(plan.steps[0].state, StepWireState::Failed(_)));
    }

    #[test]
    fn a_card_is_named_by_its_id_or_the_json_of_it() {
        assert_eq!(card_id("send").expect("bare").as_str(), "send");
        assert_eq!(card_id("\"send\"").expect("json").as_str(), "send");
        assert_eq!(card_id(""), Err(ServeFault::NoSuchCard));
    }
}
