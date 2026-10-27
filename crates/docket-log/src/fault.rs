//! What can go wrong while the log is set up. None of it stops a daemon: the log falls back and
//! says so once, in the log that did start.

use thiserror::Error;

/// A part of the requested setup that could not be had.
#[derive(Debug, Error)]
pub(crate) enum Fault {
    /// The journal's socket could not be reached; lines go to standard error instead.
    #[error("the journal is not reachable, logging to standard error: {0}")]
    Journal(#[source] std::io::Error),
    /// The span exporter could not be built; spans are dropped.
    #[cfg(feature = "otlp")]
    #[error("telemetry export is off: {0}")]
    Export(String),
}
