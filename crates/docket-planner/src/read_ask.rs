//! The planner's `quire_read` call, read part by part so a model that gets one part wrong is told
//! which one and what it looks like, instead of the turn failing on an unreadable reply.

use crate::want::settle_want;
use agent_loop::ModelOutput;
use docket_core::{Handle, HandleShape, ReadFault, ReaderAsk, ReaderTask, ReplyFault, ValueSchema};
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
    let want: ValueSchema = match args.get("want") {
        Some(want) => serde_json::from_value(settle_want(want)?).map_err(|_| ReadFault::Want)?,
        None => return Err(ReadFault::Want),
    };
    Ok(ReaderAsk { inputs, want, task })
}

/// How a read fault is told to the planner: what was wrong and the shape expected, short.
pub(crate) fn read_fault_text(fault: &ReadFault) -> String {
    match fault {
        ReadFault::Inputs => "\"inputs\" must be a list of handle numbers you were shown, such as [1, 2]".to_owned(),
        ReadFault::Task => "\"task\" must be one of classify, extract, summarise, compare".to_owned(),
        ReadFault::Want => "\"want\" is not a shape of the answer; give {\"kind\": ..., \"v\": ...} with kind one of choice, integer, date, datetime, text, record, list, such as {\"kind\": \"choice\", \"v\": [\"forward\", \"skip\"]}; choice options are ids of lowercase letters, digits and _".to_owned(),
        ReadFault::WantOption(option) => format!("option \"{option}\" is not an id and has no id to read it as: use lowercase letters, digits and _, such as \"lisbon_receipts\""),
        ReadFault::WantClash(id) => format!("two options of the choice are both \"{id}\": give each option a different id"),
        ReadFault::WantText => "text needs v: {\"max\": N}, with N the most characters (a whole number, 1 or more), such as {\"kind\": \"text\", \"v\": {\"max\": 500}}; leave v out for the default".to_owned(),
        ReadFault::NotHeld => "an input is not a handle you were shown; name only the #n handles listed".to_owned(),
        ReadFault::NotText { handle, shape } => not_text(*handle, shape),
        ReadFault::OutOfSchema(_) => "the reader's answer did not fit \"want\"; ask for a simpler shape, or a choice among options".to_owned(),
        ReadFault::Unparseable => "the reader's answer could not be read; ask for a simpler shape".to_owned(),
        ReadFault::Refused => "the reader declined to answer these inputs".to_owned(),
        ReadFault::Unavailable => "no reader could answer".to_owned(),
    }
}

/// A held thing or file is not text: how to get text from it, as a call the policy will see.
fn not_text(handle: Handle, shape: &HandleShape) -> String {
    match shape {
        HandleShape::Entity(kind) => format!(
            "#{n} is a {kind}, not text: read it first with the action that reads a {kind} (such as {kind}.read) and give quire_read the handle that call returns",
            n = handle.0
        ),
        HandleShape::File => format!(
            "#{} is a file, not text; quire_read takes only text handles",
            handle.0
        ),
        HandleShape::Text => format!("#{} is text; name it as an input again", handle.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn asked(want: Json) -> ModelOutput {
        read_output(&json!({ "inputs": [1], "task": "summarise", "want": want }))
    }

    #[test]
    fn a_text_want_without_a_length_is_a_read_with_the_default() {
        for want in [
            json!({ "kind": "text" }),
            json!({ "kind": "text", "v": {} }),
        ] {
            let ModelOutput::Read(ask) = asked(want) else {
                panic!("not a read");
            };
            assert_eq!(
                ask.want,
                ValueSchema::Text {
                    max: docket_core::CharCount(2000)
                }
            );
        }
    }

    #[test]
    fn a_text_want_with_a_bad_v_gets_the_precise_fault_not_the_generic_one() {
        let out = asked(json!({ "kind": "text", "v": { "max": "lots" } }));
        assert_eq!(
            out,
            ModelOutput::Unread(ReplyFault::Read(ReadFault::WantText))
        );
        assert!(read_fault_text(&ReadFault::WantText).starts_with("text needs v: {\"max\": N}"));
        let nothing = asked(json!({ "kind": "bogus" }));
        assert_eq!(
            nothing,
            ModelOutput::Unread(ReplyFault::Read(ReadFault::Want))
        );
    }
}
