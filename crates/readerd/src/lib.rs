//! readerd: the quarantined reader. It is a separate process with its own bus name, and the
//! only caller `intentd` lets resolve a handle. It reads untrusted text under a closed
//! [`ReaderTask`](docket_core::ReaderTask) and a [`ValueSchema`](docket_core::ValueSchema),
//! has no tools, and answers a typed value only; text inside the answer reaches the planner as
//! a new handle, never as itself.

mod answer;
mod host;
mod request;
mod serve;
mod service;

pub use answer::answer_of;
pub use host::ReaderHost;
pub use request::{class_of, reader_request, reply_shape, task_instruction};
pub use serve::{ServeFault, serve, serve_on};
pub use service::ReaderService;
