//! One read of the quarantined reader, as a step of the task: a closed-set answer is plain, text
//! a handle. It is a step in the task's history so the planner is shown what came back. The
//! companion and any smaller agent carry out the reader's effect the same way.

use crate::task::{Failure, TaskRuntime};
use agent_loop::LoopInput;
use docket_client::{ClientError, Intents, Transport as IntentsTransport};
use docket_core::{
    ActionRef, Args, CallRequest, Follow, Origin, Outcome, Preview, ReadAsk, ReadFault, ReaderAsk,
    ReplyFault, Reveal, TargetValue, Undoable, Value, WireRefusal,
};
use prov::{ActionName, Effect, Labelled};

/// The built-in provider whose actions the companion carries out itself.
pub(crate) const COMPANION_APP: &str = "org.quire.Companion";

/// Asks the reader about `ask` for the task `rt`, and says what the loop is told.
pub async fn answer_read<I: IntentsTransport>(
    intents: &Intents<I>,
    rt: &mut TaskRuntime,
    ask: ReaderAsk,
) -> Vec<LoopInput> {
    match intents
        .session_read(rt.session.clone(), ReadAsk { ask })
        .await
    {
        Err(ClientError::Refused(WireRefusal::Read(fault)))
            if !matches!(fault, ReadFault::Unavailable) =>
        {
            vec![LoopInput::ReadFailed(ReplyFault::Read(fault))]
        }
        Err(_) => {
            rt.failure = Some(Failure::Reader);
            vec![LoopInput::ModelFailed]
        }
        Ok(reveal) => {
            let value = match &reveal {
                Reveal::Plain(v) => v.clone(),
                Reveal::Handle(h) => Value::Handle(*h),
            };
            let outcome = Outcome {
                value: Some(Labelled {
                    value,
                    label: docket_planner::planner_label(),
                }),
                said: None,
                show: Preview::None,
                undo: Undoable::No,
                follow: Follow::Nothing,
            };
            if let Some(call) = read_call() {
                rt.record_call(&call, Effect::Read, Ok(outcome));
            }
            vec![LoopInput::ReadAnswered(reveal)]
        }
    }
}

/// The step the planner is shown for a read: the reader is a step like any other.
fn read_call() -> Option<CallRequest> {
    Some(CallRequest {
        action: ActionRef {
            app: porter_core::AppName::parse(COMPANION_APP).ok()?,
            name: ActionName::parse("companion.read").ok()?,
        },
        target: TargetValue::Nothing,
        args: Args::new(),
        origin: Origin::Companion,
    })
}
