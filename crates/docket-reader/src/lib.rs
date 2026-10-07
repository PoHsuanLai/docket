//! The quarantined reader, portable. It reads untrusted text under a closed
//! [`ReaderTask`](docket_core::ReaderTask) and a [`ValueSchema`](docket_core::ValueSchema), has no
//! tools, and answers a typed value only; text inside the answer reaches the planner as a new
//! handle, never as itself. Moved out of readerd so the desktop's process and an app-hosted agent
//! share the request, the answer's reading and the model call; readerd keeps the bus.
//!
//! - [`reader_request`], [`task_instruction`], [`reply_shape`], [`class_of`]: what the model is asked.
//! - [`answer_of`]: the reply read back under the schema.
//! - [`read`]: one read over a Transport; [`TransportReader`]: the in-process `Reader`.

mod answer;
mod read;
mod request;

pub use answer::answer_of;
pub use read::{TransportReader, read};
pub use request::{class_of, reader_request, reply_shape, task_instruction};
