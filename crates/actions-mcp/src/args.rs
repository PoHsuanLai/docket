//! JSON arguments to typed ones, by each parameter's declared type: `docket_core`'s one reader
//! (`args_from_json`), which the companion's tool calls use too. Every value comes out labelled with
//! the label the edge gives it (`mcp_label(client)`).

pub use docket_core::{TARGET_KEY, args_from_json as read_call, target_from_json as target};
