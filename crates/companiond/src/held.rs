//! A call the loop did not make because the planner had made it before and nothing had changed.
//! It is a line in the task's history, so the planner's next view says why it was not run, and
//! nothing else: the router is not asked, no card is drawn and the ledger does not count it.

use crate::runtime::Companiond;
use docket_client::Transport as IntentsTransport;
use docket_core::{CallId, CallRequest, Held, StepEnd, StepLine, StepShown};
use porter_client::Transport as InferTransport;
use prov::{Effect, TaskId};

impl<P: InferTransport, I: IntentsTransport> Companiond<P, I> {
    /// Tells the task's history that `call` was not run, and why.
    pub(crate) fn hold(&mut self, task: &TaskId, call: &CallRequest, why: Held) {
        let effect = self
            .planner
            .catalogue()
            .of(&call.action)
            .map_or(Effect::Read, |t| t.decl.effect);
        if let Some(rt) = self.runtimes.get_mut(task) {
            let id = CallId(rt.next_call);
            rt.next_call += 1;
            rt.history.push(StepLine {
                call: id,
                action: call.action.clone(),
                effect,
                end: StepEnd::Held(why),
                shown: StepShown::Full,
            });
        }
    }
}
