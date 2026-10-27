//! The one log setup of the daemons. A daemon's `main` calls [`init`] once; the code that fails
//! or refuses something says so with a `tracing` event (`tracing::warn!(?fault, "what happened")`).
//! Events go to standard error, one line each, which systemd keeps in the journal. When the
//! journal is what listens (`JOURNAL_STREAM` is set) each line starts with the syslog priority
//! as `<N>`, so `journalctl -p warning` works.
//!
//! What an event may carry is the caller's to keep short: names, counts, typed reasons. Never a
//! prompt, an utterance, a message body, a key or a token.
//!
//! No subscriber crate is used: this is the whole of it, over the `tracing` the workspace
//! already builds.
//!
//! # Span export (feature `otlp`)
//!
//! With the `otlp` feature, `init` also reads `OTEL_EXPORTER_OTLP_ENDPOINT` (or
//! `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`, used as given), `OTEL_SERVICE_NAME` and
//! `OTEL_SDK_DISABLED`, once. When an `http://` endpoint is named, spans are recorded and sent in
//! batches as OTLP/HTTP JSON by a small exporter written here over `std` networking. An endpoint
//! that is not `http://` is refused with one warning and export stays off; a person who wants TLS
//! or a remote backend runs a local collector. Without the feature none of this is built.
//!
//! A span is exported as the fields it declared, by key and value, and nothing else: no events and
//! no event messages. As for events, callers must never put content on a span: no prompt,
//! utterance, message body, argument, result, key or token. Names, ids, counts and typed reasons
//! only.

#[cfg(feature = "otlp")]
mod otlp;

use std::fmt::{self, Write as _};
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

/// Who is reading standard error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reader {
    /// The journal: lines carry their priority.
    Journal,
    /// A terminal or a file: lines are plain.
    Plain,
}

/// The events of one daemon, as lines on standard error.
#[derive(Debug)]
struct Lines {
    daemon: &'static str,
    reader: Reader,
    spans: AtomicU64,
    /// Records spans for export; none when export is off.
    #[cfg(feature = "otlp")]
    recorder: Option<otlp::Recorder>,
}

/// The `key=value` text of an event's fields; the message comes first and bare.
#[derive(Debug, Default)]
struct Fields(String);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let _ = match field.name() {
            "message" => write!(self.0, " {value:?}"),
            name => write!(self.0, " {name}={value:?}"),
        };
    }
}

/// The syslog priority of a level.
fn priority(level: &Level) -> u8 {
    match *level {
        Level::ERROR => 3,
        Level::WARN => 4,
        Level::INFO => 6,
        _ => 7,
    }
}

/// One line: `[<priority>]daemon: LEVEL fields`.
fn render(reader: Reader, daemon: &str, level: &Level, fields: &str) -> String {
    let prefix = match reader {
        Reader::Journal => format!("<{}>", priority(level)),
        Reader::Plain => String::new(),
    };
    format!("{prefix}{daemon}: {level}{fields}")
}

impl Subscriber for Lines {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        *metadata.level() <= Level::INFO
    }

    fn new_span(&self, _attrs: &Attributes<'_>) -> Id {
        let id = Id::from_u64(self.spans.fetch_add(1, Ordering::Relaxed) + 1);
        #[cfg(feature = "otlp")]
        {
            if let Some(recorder) = &self.recorder {
                recorder.new_span(_attrs, &id);
            }
        }
        id
    }

    fn record(&self, _id: &Id, _values: &Record<'_>) {
        #[cfg(feature = "otlp")]
        {
            if let Some(recorder) = &self.recorder {
                recorder.record(_id, _values);
            }
        }
    }

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let line = render(
            self.reader,
            self.daemon,
            event.metadata().level(),
            &fields.0,
        );
        let _ = writeln!(std::io::stderr().lock(), "{line}");
    }

    fn enter(&self, _id: &Id) {
        #[cfg(feature = "otlp")]
        {
            if let Some(recorder) = &self.recorder {
                recorder.enter(_id);
            }
        }
    }

    fn exit(&self, _id: &Id) {
        #[cfg(feature = "otlp")]
        {
            if let Some(recorder) = &self.recorder {
                recorder.exit(_id);
            }
        }
    }

    #[cfg(feature = "otlp")]
    fn clone_span(&self, id: &Id) -> Id {
        if let Some(recorder) = &self.recorder {
            recorder.clone_span(id);
        }
        id.clone()
    }

    #[cfg(feature = "otlp")]
    fn try_close(&self, id: Id) -> bool {
        match &self.recorder {
            Some(recorder) => recorder.try_close(&id),
            None => true,
        }
    }
}

/// Starts logging for the daemon `daemon` (its name begins every line). Events at `INFO` and
/// above are kept. A second call, or a process that already has a subscriber, changes nothing.
pub fn init(daemon: &'static str) {
    let reader = match std::env::var_os("JOURNAL_STREAM") {
        Some(_) => Reader::Journal,
        None => Reader::Plain,
    };
    #[cfg(feature = "otlp")]
    let (recorder, refused) = otlp::start(daemon);
    let lines = Lines {
        daemon,
        reader,
        spans: AtomicU64::new(0),
        #[cfg(feature = "otlp")]
        recorder,
    };
    let _ = tracing::subscriber::set_global_default(lines);
    #[cfg(feature = "otlp")]
    {
        if let Some(why) = refused {
            tracing::warn!(%why, "telemetry export is off");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_has_the_priority_only_for_the_journal() {
        let rows = [
            (Reader::Journal, Level::WARN, "<4>voiced: WARN refused"),
            (Reader::Journal, Level::ERROR, "<3>voiced: ERROR refused"),
            (Reader::Journal, Level::INFO, "<6>voiced: INFO refused"),
            (Reader::Plain, Level::WARN, "voiced: WARN refused"),
        ];
        for (reader, level, want) in rows {
            assert_eq!(render(reader, "voiced", &level, " refused"), want);
        }
    }
}
