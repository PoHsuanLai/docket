//! One writer per thing. A helper's job owns the things it was given (its seat names them): while
//! its session lives and its task has not ended, a write to one of them from any other session is
//! refused with `DenyCode::OwnedByAnotherJob`. The person's own acts through an app are not calls
//! to the router and are not held back, and a read of an owned thing is free to anyone.

use crate::session::SessionState;
use crate::state::{RouterState, SessionRecord};
use crate::tasks::TaskState;
use docket_core::{CallRequest, TargetValue, Value};
use prov::EntityId;

/// The things a call touches: its targets and the things among its arguments.
pub(crate) fn touched(request: &CallRequest) -> Vec<EntityId> {
    let mut things: Vec<EntityId> = match &request.target {
        TargetValue::Entities(es) => es.clone(),
        TargetValue::Nothing
        | TargetValue::Handles(_)
        | TargetValue::Text(_)
        | TargetValue::Files(_) => Vec::new(),
    };
    for arg in request.args.values() {
        match &arg.value {
            Value::Entity(e) => things.push(e.clone()),
            Value::Entities(es) => things.extend(es.iter().cloned()),
            _ => {}
        }
    }
    things
}

/// Whether the session is still a live job: open, and its task not over.
fn alive(st: &RouterState, record: &SessionRecord) -> bool {
    !matches!(record.state, SessionState::Closed(_))
        && st
            .tasks
            .get(&record.task)
            .is_none_or(|t| !matches!(t.state, TaskState::Ended(_)))
}

/// Whether a live job of another session owns one of `things`.
pub(crate) fn owned_by_another(
    st: &RouterState,
    mine: &SessionRecord,
    things: &[EntityId],
) -> bool {
    st.sessions
        .values()
        .filter(|other| other.task != mine.task && alive(st, other))
        .filter_map(|other| other.helper.as_ref())
        .any(|seat| seat.owns.iter().any(|owned| things.contains(owned)))
}
