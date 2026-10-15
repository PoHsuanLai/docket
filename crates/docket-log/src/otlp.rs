//! Telemetry export, behind the `otlp` feature. See the TODO in `Cargo.toml`: the exporter is not
//! built yet, so asking for one is reported, never silently ignored.

/// Where spans go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Export {
    /// Nowhere: the default.
    Off,
    /// To this OTLP endpoint.
    To(String),
}

/// The export an `OTEL_EXPORTER_OTLP_ENDPOINT` value asks for: none when unset or blank.
pub(crate) fn export_from(value: Option<&str>) -> Export {
    match value.map(str::trim) {
        Some(endpoint) if !endpoint.is_empty() => Export::To(endpoint.to_owned()),
        _ => Export::Off,
    }
}

/// Reads the environment (the daemon's `main` is the caller, through `init`) and says so when an
/// export was asked for.
pub(crate) fn note() {
    let value = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();
    if let Export::To(endpoint) = export_from(value.as_deref()) {
        tracing::warn!(%endpoint, "telemetry export was asked for, but this build has no exporter");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_non_blank_endpoint_turns_export_on() {
        let rows = [
            (None, Export::Off),
            (Some(""), Export::Off),
            (Some("  "), Export::Off),
            (
                Some("http://127.0.0.1:4317"),
                Export::To("http://127.0.0.1:4317".into()),
            ),
        ];
        for (value, want) in rows {
            assert_eq!(export_from(value), want, "{value:?}");
        }
    }
}
