//! Span export to an OTLP/HTTP collector, over the OpenTelemetry crates.

use crate::config::{Config, Export};
use crate::fault::Fault;
use crate::install::BoxedLayer;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::Layer as _;
use tracing_subscriber::filter::{FilterExt as _, filter_fn};

/// Export is on when a collector is named and the SDK is not disabled.
pub(crate) fn export_from_env() -> Export {
    let named = [
        "OTEL_EXPORTER_OTLP_ENDPOINT",
        "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
    ]
    .iter()
    .any(|name| std::env::var_os(name).is_some());
    let disabled = std::env::var("OTEL_SDK_DISABLED").is_ok_and(|v| v.eq_ignore_ascii_case("true"));
    match (named, disabled) {
        (true, false) => Export::Otlp,
        _ => Export::Off,
    }
}

/// Adds the span layer to `layers` when export is on; the provider is what the guard shuts down.
pub(crate) fn start(
    config: &Config,
    layers: &mut Vec<BoxedLayer>,
    faults: &mut Vec<Fault>,
) -> Option<SdkTracerProvider> {
    if config.export == Export::Off {
        return None;
    }
    match build(config.daemon) {
        Ok(provider) => {
            let tracer = provider.tracer(config.daemon);
            // Spans only: an event is not sent, so a message never leaves the machine.
            let spans_only = filter_fn(|meta| meta.is_span());
            let layer = tracing_opentelemetry::layer()
                .with_tracer(tracer)
                .with_location(false)
                .with_filter(spans_only.and(config.filter.clone()))
                .boxed();
            layers.push(layer);
            Some(provider)
        }
        Err(fault) => {
            faults.push(fault);
            None
        }
    }
}

fn build(daemon: &'static str) -> Result<SdkTracerProvider, Fault> {
    let exporter = SpanExporter::builder()
        .with_http()
        .build()
        .map_err(|why| Fault::Export(why.to_string()))?;
    let resource = match std::env::var_os("OTEL_SERVICE_NAME") {
        Some(_) => Resource::builder().build(),
        None => Resource::builder().with_service_name(daemon).build(),
    };
    Ok(SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build())
}
