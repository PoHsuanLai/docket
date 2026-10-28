//! The names of the companion's own actions, the rule that the helper ones are the companion's
//! planners' alone, and the things that stand for its tasks.

use docket_permissions_core::COMPANION_APP;
use docket_vocab::ActionRef;
use porter_core::AppName;
use prov::{EntityId, EntityKey, EntityKind, TaskId};

/// The kind of thing a task is.
const TASK_KIND: &str = "companion.task";

/// The assistant's one action that adds a limit.
pub const LIMIT_ADD: &str = "companion.limit.add";

/// Gives a job to a helper by its name.
pub const HELPER_POST_JOB: &str = "companion.helper.post_job";
/// Proposes a helper to the person, who confirms it on a sheet.
pub const HELPER_PROPOSE: &str = "companion.helper.propose";
/// Proposes a routine to the person, who accepts or declines it on a sheet.
pub const ROUTINE_PROPOSE: &str = "companion.routine.propose";
/// Shows the person a brief to check and change; what they confirm becomes their words in the
/// session. The router answers it itself (see `brief`), since only it sees the sheet's words.
pub const BRIEF_CONFIRM: &str = "companion.brief.confirm";

/// Whether `action` is one of the helper actions: only the companion's own planners may call
/// them, however an app or an outside client names them.
pub fn is_helper_action(action: &ActionRef) -> bool {
    action.app.as_str() == COMPANION_APP
        && matches!(
            action.name.as_str(),
            HELPER_POST_JOB | HELPER_PROPOSE | ROUTINE_PROPOSE | BRIEF_CONFIRM
        )
}

fn name() -> AppName {
    AppName::parse(COMPANION_APP).expect("`org.quire.Companion` is a valid app name")
}

/// The thing of kind `companion.task` that stands for `task`.
pub fn task_entity(task: &TaskId) -> Option<EntityId> {
    Some(EntityId {
        app: name(),
        kind: EntityKind::parse(TASK_KIND).ok()?,
        key: EntityKey::parse(task.as_str()).ok()?,
    })
}

/// The task an entity of kind `companion.task` names.
pub fn task_of(entity: &EntityId) -> Option<TaskId> {
    (entity.app == name() && entity.kind.as_str() == TASK_KIND)
        .then(|| TaskId::parse(entity.key.as_str()).ok())
        .flatten()
}
