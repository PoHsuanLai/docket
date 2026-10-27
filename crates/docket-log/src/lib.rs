//! The one log setup of the daemons. A daemon's `main` builds a [`Config`] and calls [`init`]
//! once; the code that fails or refuses something says so with a `tracing` event
//! (`tracing::warn!(?fault, "what happened")`).
//!
//! The subscriber is the standard one: `tracing-subscriber`'s `fmt` layer writing a line per
//! event to standard error, or `tracing-journald` when the journal listens (`JOURNAL_STREAM` is
//! set), so `journalctl -p warning` works and the daemon name is the journal's
//! `SYSLOG_IDENTIFIER`. Which events are kept is a `RUST_LOG`-style filter (`info` by default).
//!
//! What an event may carry is the caller's to keep short: names, counts, typed reasons. Never a
//! prompt, an utterance, a message body, a key or a token.
//!
//! # Span export (feature `otlp`)
//!
//! With the `otlp` feature, [`init`] also sends spans to an OTLP/HTTP collector when
//! `OTEL_EXPORTER_OTLP_ENDPOINT` (or `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`) is set, unless
//! `OTEL_SDK_DISABLED` is `true`. A span is exported as the fields it declared and nothing else:
//! events are not sent. Callers must never put content on a span: no prompt, utterance, message
//! body, argument, result, key or token. Names, ids, counts and typed reasons only.

mod config;
mod fault;
mod install;
#[cfg(feature = "otlp")]
mod otlp;

#[cfg(feature = "otlp")]
pub use config::Export;
pub use config::{Config, Sink};
pub use install::{Guard, init};
