//! `quire_finish`'s `show`: the text handles the person is shown in the answer. The planner names
//! them and never reads them; the host puts them in the answer as handles, which the screen
//! resolves as quoted content (`Session.Display`), so what a handle holds is never taken for the
//! planner's words or for an instruction.

use crate::planner::PlanFault;
use agent_loop::ModelOutput;
use docket_core::{ArgsFault, Handle, HandleShape, ParamName, PlannerView, ReplyFault, Why};
use porter_infer::ToolCallPart;
use serde_json::Value as Json;

/// The most handles one finish may show.
pub(crate) const MOST_SHOWN: usize = 8;

/// The name of the list in `quire_finish`'s arguments.
const SHOW: &str = "show";

fn wrong(why: Why) -> ReplyFault {
    match ParamName::parse(SHOW) {
        Ok(param) => ReplyFault::Args(ArgsFault::Wrong { param, why }),
        Err(_) => ReplyFault::NotJson,
    }
}

/// The handles `args` names to show: none when it names none; else each a text handle of the
/// view (an unknown handle, one of another session, or a thing that is not text is out of range),
/// each once, at most [`MOST_SHOWN`].
fn shown(args: &Json, view: &PlannerView) -> Result<Vec<Handle>, ReplyFault> {
    let Some(list) = args.get(SHOW) else {
        return Ok(Vec::new());
    };
    let items = list.as_array().ok_or_else(|| wrong(Why::Type))?;
    if items.len() > MOST_SHOWN {
        return Err(wrong(Why::Range));
    }
    let mut out: Vec<Handle> = Vec::new();
    for item in items {
        let handle = item.as_u64().map(Handle).ok_or_else(|| wrong(Why::Type))?;
        let text = view
            .handles
            .iter()
            .any(|c| c.handle == handle && c.shape == HandleShape::Text);
        if !text {
            return Err(wrong(Why::Range));
        }
        if !out.contains(&handle) {
            out.push(handle);
        }
    }
    Ok(out)
}

/// A `quire_finish` as the end of the task and the handles to show; a `show` that names anything
/// but text handles of the view is told to the model, and the task does not end.
pub(crate) fn finish_output(
    call: &ToolCallPart,
    view: &PlannerView,
) -> Result<(ModelOutput, Vec<Handle>), PlanFault> {
    let args: Json = serde_json::from_str(call.args.as_str()).map_err(|_| PlanFault::Unreadable)?;
    Ok(match shown(&args, view) {
        Ok(show) => (ModelOutput::Finish, show),
        Err(fault) => (ModelOutput::Unread(fault), Vec::new()),
    })
}
