//! What the router remembers and shows. The session record and the lock around everything the
//! router keeps (`state`), the session machine with its breaker pause (`session`), the handle
//! table that keeps untrusted text away from planners (`handles`, `handle_views`), tasks (`tasks`),
//! the write-ahead log of a session (`wal`) and the session rebuilt from it (`rebuild`), the
//! undo journal (`journal`), the manifest registry (`registry`), the labels a value carries
//! (`labels`), who is asking (`who`), the sheets open now (`asks`), what a session owns
//! (`owned`), the relation between Spaces (`spacing`) and the shadow index machine (`index`).
//!
//! Pure: no clock, bus, file or runtime is reached here. `docket-router` ties these to its
//! seams.

pub mod asks;
mod convert;
pub mod handle_views;
pub mod handles;
pub mod index;
pub mod journal;
pub mod labels;
pub mod owned;
pub mod rebuild;
mod recording;
pub mod registry;
pub mod session;
pub mod spacing;
pub mod spoken;
pub mod state;
pub mod tasks;
pub mod wal;
pub mod who;
