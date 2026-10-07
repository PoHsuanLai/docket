//! The planner's `quire_read` call, read part by part so a model that gets one part wrong is told
//! which one and what it looks like, instead of the turn failing on an unreadable reply.

use agent_loop::ModelOutput;
use docket_core::{Handle, ReadFault, ReaderAsk, ReaderTask, ReplyFault, ValueSchema};
use serde_json::Value as Json;

/// The most handles one read may name: a model that lists hundreds has not chosen.
const MOST_INPUTS: usize = 32;

/// The read the model asked for, or the part of it that was wrong.
pub(crate) fn read_output(args: &Json) -> ModelOutput {
    match read_ask(args) {
        Ok(ask) => ModelOutput::Read(ask),
        Err(fault) => ModelOutput::Unread(ReplyFault::Read(fault)),
    }
}

fn part<T: serde::de::DeserializeOwned>(
    args: &Json,
    name: &str,
    fault: ReadFault,
) -> Result<T, ReadFault> {
    args.get(name)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .ok_or(fault)
}

fn read_ask(args: &Json) -> Result<ReaderAsk, ReadFault> {
    let inputs: Vec<Handle> = part(args, "inputs", ReadFault::Inputs)?;
    if inputs.is_empty() || inputs.len() > MOST_INPUTS {
        return Err(ReadFault::Inputs);
    }
    let task: ReaderTask = part(args, "task", ReadFault::Task)?;
    let want: ValueSchema = part(args, "want", ReadFault::Want)?;
    Ok(ReaderAsk { inputs, want, task })
}

/// The tool's `want` parameter: the closed shapes of an answer, as the reader takes them.
pub(crate) fn want_schema() -> Json {
    serde_json::json!({
        "type": "object",
        "description": "The shape of the answer, as {\"kind\": ..., \"v\": ...}. Kinds: choice (v: list of option strings), integer (v: {min, max}), date, datetime, text (v: {max}), record (v: list of [name, shape]), list (v: {of: shape, max}). Example: {\"kind\": \"choice\", \"v\": [\"forward\", \"skip\"]}",
        "properties": {
            "kind": { "enum": ["choice", "integer", "date", "datetime", "text", "record", "list"] },
            "v": {},
        },
        "required": ["kind"],
    })
}

/// How a read fault is told to the planner: what was wrong and the shape expected, short.
pub(crate) fn read_fault_text(fault: &ReadFault) -> String {
    match fault {
        ReadFault::Inputs => "\"inputs\" must be a list of handle numbers you were shown, such as [1, 2]".to_owned(),
        ReadFault::Task => "\"task\" must be one of classify, extract, summarise, compare".to_owned(),
        ReadFault::Want => "\"want\" is not a shape of the answer; give {\"kind\": ..., \"v\": ...} with kind one of choice, integer, date, datetime, text, record, list, such as {\"kind\": \"choice\", \"v\": [\"forward\", \"skip\"]}".to_owned(),
        ReadFault::NotHeld => "an input is not a handle you were shown; name only the #n handles listed".to_owned(),
        ReadFault::OutOfSchema(_) => "the reader's answer did not fit \"want\"; ask for a simpler shape, or a choice among options".to_owned(),
        ReadFault::Unparseable => "the reader's answer could not be read; ask for a simpler shape".to_owned(),
        ReadFault::Refused => "the reader declined to answer these inputs".to_owned(),
        ReadFault::Unavailable => "no reader could answer".to_owned(),
    }
}
