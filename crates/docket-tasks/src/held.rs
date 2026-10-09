//! A call the loop did not make because the planner had made it before and nothing had changed.
//! It is a line in the task's history, so the planner's next view says why it was not run, and
//! nothing else: the router is not asked, no card is drawn and the ledger does not count it.

use crate::runtime::Companion;
use crate::seams::{Now, Surface};
use crate::task::TaskRuntime;
use docket_client::Transport as IntentsTransport;
use docket_core::{CallId, CallRequest, Held, ReplyFault, StepEnd, StepLine, StepShown};
use porter_client::Transport as InferTransport;
use prov::{Effect, TaskId};

impl TaskRuntime {
    /// Tells the history that `call`, of `effect`, was not run, and why.
    pub fn hold(&mut self, call: &CallRequest, effect: Effect, why: Held) {
        let id = CallId(self.next_call);
        self.next_call += 1;
        self.history.push(StepLine {
            call: id,
            action: call.action.clone(),
            effect,
            end: StepEnd::Held(why),
            shown: StepShown::Full,
            with: call.handles(),
        });
    }

    /// Tells the history that the planner's last reply could not be read, and why.
    pub fn unread(&mut self, fault: ReplyFault) {
        let id = CallId(self.next_call);
        self.next_call += 1;
        self.history.extend(StepLine::unread(id, fault));
    }
}

impl<P: InferTransport, I: IntentsTransport, K: Now, S: Surface> Companion<P, I, K, S> {
    /// Tells the task's history that `call` was not run, and why.
    pub(crate) fn hold(&mut self, task: &TaskId, call: &CallRequest, why: Held) {
        let effect = self
            .planner
            .catalogue()
            .of(&call.action)
            .map_or(Effect::Read, |t| t.decl.effect);
        if let Some(rt) = self.runtimes.get_mut(task) {
            rt.hold(call, effect, why);
        }
    }

    /// Tells the task's history that the planner's last reply could not be read, and why.
    pub(crate) fn unread(&mut self, task: &TaskId, fault: ReplyFault) {
        if let Some(rt) = self.runtimes.get_mut(task) {
            rt.unread(fault);
        }
    }
}
