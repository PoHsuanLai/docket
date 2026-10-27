//! Builds the subscriber from a [`Config`] and makes it the process's default.

use crate::config::{Config, Sink};
use crate::fault::Fault;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{Layer, Registry};

/// A layer over the bare registry, boxed so the sink chooses it at run time.
pub(crate) type BoxedLayer = Box<dyn Layer<Registry> + Send + Sync>;

/// Keep it alive until the daemon exits: dropping it sends the spans still queued.
#[must_use = "dropping the guard at once loses the spans still queued"]
#[derive(Debug)]
pub struct Guard {
    #[cfg(feature = "otlp")]
    provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
}

#[cfg(feature = "otlp")]
impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(provider) = self.provider.take() {
            let _ = provider.shutdown();
        }
    }
}

/// Starts logging as `config` says. A process that already has a subscriber keeps it and this
/// changes nothing. A part of the setup that cannot be had (the journal, the exporter) is
/// replaced by the plain one and reported by one warning.
pub fn init(config: &Config) -> Guard {
    let mut faults = Vec::new();
    let mut layers = vec![sink_layer(config, &mut faults)];
    #[cfg(feature = "otlp")]
    let provider = crate::otlp::start(config, &mut layers, &mut faults);
    let installed = Registry::default().with(layers).try_init().is_ok();
    if installed {
        for fault in &faults {
            tracing::warn!(%fault, "log setup fell back");
        }
    }
    Guard {
        #[cfg(feature = "otlp")]
        provider: provider.filter(|_| installed),
    }
}

/// The layer that writes events: the journal's, or the plain one on standard error.
fn sink_layer(config: &Config, faults: &mut Vec<Fault>) -> BoxedLayer {
    match config.sink {
        Sink::Journal => match journal_layer(config.daemon, config.filter.clone()) {
            Ok(layer) => layer,
            Err(fault) => {
                faults.push(fault);
                stderr_layer(config.filter.clone())
            }
        },
        Sink::Stderr => stderr_layer(config.filter.clone()),
    }
}

fn journal_layer(daemon: &str, filter: Targets) -> Result<BoxedLayer, Fault> {
    let layer = tracing_journald::layer()
        .map_err(Fault::Journal)?
        .with_syslog_identifier(daemon.to_owned());
    Ok(layer.with_filter(filter).boxed())
}

fn stderr_layer(filter: Targets) -> BoxedLayer {
    tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .with_filter(filter)
        .boxed()
}
