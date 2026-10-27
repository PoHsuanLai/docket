//! What a daemon asks of its log: who it is, where lines go, which events are kept.

use std::ffi::OsStr;
use tracing::metadata::LevelFilter;
use tracing_subscriber::filter::Targets;

/// Where the lines go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sink {
    /// The journal, through its socket, with the journal's own fields and priorities.
    Journal,
    /// Standard error: a terminal or a file, plain lines.
    Stderr,
}

/// Whether spans leave the machine.
#[cfg(feature = "otlp")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Export {
    /// Spans are made and dropped.
    Off,
    /// Spans go to the OTLP/HTTP collector the `OTEL_*` variables name.
    Otlp,
}

/// The typed setup of one daemon's log.
#[derive(Debug, Clone)]
pub struct Config {
    /// The daemon's name: the journal's `SYSLOG_IDENTIFIER` and the OTLP `service.name`.
    pub daemon: &'static str,
    /// Where lines go.
    pub sink: Sink,
    /// Which events and spans are kept, by target and level.
    pub filter: Targets,
    /// Whether spans are exported.
    #[cfg(feature = "otlp")]
    pub export: Export,
}

impl Config {
    /// The setup the environment asks for: the journal when `JOURNAL_STREAM` is set, the
    /// `RUST_LOG` filter, and span export when the `OTEL_*` variables name a collector.
    /// Only a daemon's `main` should call this.
    pub fn from_env(daemon: &'static str) -> Self {
        let journal_stream = std::env::var_os("JOURNAL_STREAM");
        let rust_log = std::env::var("RUST_LOG").ok();
        Self::from_vars(daemon, journal_stream.as_deref(), rust_log.as_deref()).with_env_export()
    }

    /// Turns span export on when the `OTEL_*` variables name a collector.
    #[cfg(feature = "otlp")]
    fn with_env_export(self) -> Self {
        Self {
            export: crate::otlp::export_from_env(),
            ..self
        }
    }

    /// Without the `otlp` feature there is nothing to turn on.
    #[cfg(not(feature = "otlp"))]
    fn with_env_export(self) -> Self {
        self
    }

    /// The setup for the two variables that decide the sink and the filter. A filter that does
    /// not parse is ignored, and the default (`info` and above) stands.
    pub fn from_vars(
        daemon: &'static str,
        journal_stream: Option<&OsStr>,
        rust_log: Option<&str>,
    ) -> Self {
        let sink = match journal_stream {
            Some(_) => Sink::Journal,
            None => Sink::Stderr,
        };
        let filter = rust_log
            .and_then(|text| text.parse::<Targets>().ok())
            .unwrap_or_else(default_filter);
        Self {
            daemon,
            sink,
            filter,
            #[cfg(feature = "otlp")]
            export: Export::Off,
        }
    }
}

/// `info` and above, from every target.
fn default_filter() -> Targets {
    Targets::new().with_default(LevelFilter::INFO)
}
