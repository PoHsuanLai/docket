//! docket's memory seam, pure: the router's audit records as almanac's events, and the bounded
//! queue the router appends them to. No transport and no IO: `docket-memory` holds the glue that
//! drains the queue into an almanac `Memory`.
//!
//! - [`record_of`], [`kind_tag_of`], [`space_named_by`]: the router's audit records as almanac's
//!   events.
//! - [`QueuedSink`]: the bounded event sink the host drains.

mod record;
mod sink;

pub use record::{kind_tag_of, record_of, space_named_by};
pub use sink::{QUEUE_LIMIT, QueuedSink};
