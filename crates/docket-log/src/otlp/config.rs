//! What the environment asks of the exporter: where spans go and under which service name.
//! Read once by `init`; every rule here is a pure function over a lookup, so tests need no
//! environment.

use url::{Host, ParseError, Url};

/// The path the traces signal is posted to when only the base endpoint is given.
const TRACES_PATH: &str = "/v1/traces";

/// Where one collector listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Endpoint {
    /// The host name or address, without the brackets of an IPv6 literal.
    pub host: String,
    pub port: u16,
    /// The request path, starting with `/`.
    pub path: String,
}

impl Endpoint {
    /// The `Host` header value: `host:port`, with brackets round an IPv6 literal.
    pub(crate) fn host_header(&self) -> String {
        match self.host.contains(':') {
            true => format!("[{}]:{}", self.host, self.port),
            false => format!("{}:{}", self.host, self.port),
        }
    }
}

/// Which variable named the endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    /// `OTEL_EXPORTER_OTLP_ENDPOINT`: the signal path is appended.
    Base,
    /// `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`: used as given.
    Traces,
}

/// Why an endpoint was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum EndpointError {
    #[error(
        "only http:// endpoints are accepted; run a local collector for TLS or a remote backend"
    )]
    NotHttp,
    #[error("the endpoint has no host")]
    NoHost,
    #[error("the host is not a name or an address")]
    BadHost,
    #[error("the port is not a number from 1 to 65535")]
    BadPort,
    #[error("the endpoint has a query, a fragment, user information or a space")]
    Extras,
}

/// The endpoint a URL names. Strict: `http://` only, then `host[:port]` and an optional path, no
/// query, fragment, user information or white space. A missing port is 80, as for any `http://`
/// URL. The URL is read as the WHATWG standard reads it, by the `url` crate.
pub(crate) fn parse_endpoint(raw: &str, source: Source) -> Result<Endpoint, EndpointError> {
    let raw = raw.trim();
    if raw.contains(char::is_whitespace) {
        return Err(EndpointError::Extras);
    }
    let url = Url::parse(raw).map_err(|why| match why {
        ParseError::EmptyHost => EndpointError::NoHost,
        ParseError::InvalidPort => EndpointError::BadPort,
        ParseError::RelativeUrlWithoutBase => EndpointError::NotHttp,
        _ => EndpointError::BadHost,
    })?;
    if url.scheme() != "http" {
        return Err(EndpointError::NotHttp);
    }
    if url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(EndpointError::Extras);
    }
    let host = match url.host().ok_or(EndpointError::NoHost)? {
        Host::Domain(name) => name.to_owned(),
        Host::Ipv4(address) => address.to_string(),
        Host::Ipv6(address) => address.to_string(),
    };
    let port = url
        .port_or_known_default()
        .filter(|port| *port != 0)
        .ok_or(EndpointError::BadPort)?;
    let path = match source {
        Source::Traces => url.path().to_owned(),
        Source::Base => format!("{}{TRACES_PATH}", url.path().trim_end_matches('/')),
    };
    Ok(Endpoint { host, port, path })
}

/// An exporter that is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Settings {
    pub endpoint: Endpoint,
    /// The `service.name` resource attribute.
    pub service: String,
}

/// What the environment asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Plan {
    /// Nothing: the default, or `OTEL_SDK_DISABLED=true`.
    Off,
    /// An endpoint was named and is not acceptable.
    Refused(EndpointError),
    On(Settings),
}

/// The plan for the daemon `daemon`, from a lookup of environment variables. The traces
/// endpoint wins over the base endpoint; blank values count as unset.
pub(crate) fn plan(daemon: &str, get: impl Fn(&str) -> Option<String>) -> Plan {
    let value = |key: &str| {
        get(key)
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
    };
    if value("OTEL_SDK_DISABLED").is_some_and(|text| text.eq_ignore_ascii_case("true")) {
        return Plan::Off;
    }
    let asked = match value("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT") {
        Some(url) => Some((url, Source::Traces)),
        None => value("OTEL_EXPORTER_OTLP_ENDPOINT").map(|url| (url, Source::Base)),
    };
    let Some((url, source)) = asked else {
        return Plan::Off;
    };
    match parse_endpoint(&url, source) {
        Ok(endpoint) => Plan::On(Settings {
            endpoint,
            service: value("OTEL_SERVICE_NAME").unwrap_or_else(|| daemon.to_owned()),
        }),
        Err(why) => Plan::Refused(why),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(host: &str, port: u16, path: &str) -> Endpoint {
        Endpoint {
            host: host.into(),
            port,
            path: path.into(),
        }
    }

    #[test]
    fn urls_parse_strictly() {
        let rows = [
            (
                "http://127.0.0.1:4318",
                Source::Base,
                Ok(endpoint("127.0.0.1", 4318, "/v1/traces")),
            ),
            (
                "http://127.0.0.1:4318/",
                Source::Base,
                Ok(endpoint("127.0.0.1", 4318, "/v1/traces")),
            ),
            (
                "  http://otel:4318/prefix/  ",
                Source::Base,
                Ok(endpoint("otel", 4318, "/prefix/v1/traces")),
            ),
            (
                "HTTP://localhost",
                Source::Base,
                Ok(endpoint("localhost", 80, "/v1/traces")),
            ),
            (
                "http://[::1]:4318",
                Source::Base,
                Ok(endpoint("::1", 4318, "/v1/traces")),
            ),
            (
                "http://[::1]",
                Source::Base,
                Ok(endpoint("::1", 80, "/v1/traces")),
            ),
            (
                "http://h:4318",
                Source::Traces,
                Ok(endpoint("h", 4318, "/")),
            ),
            (
                "http://h:4318/custom/path",
                Source::Traces,
                Ok(endpoint("h", 4318, "/custom/path")),
            ),
            ("https://h:4318", Source::Base, Err(EndpointError::NotHttp)),
            ("grpc://h:4317", Source::Base, Err(EndpointError::NotHttp)),
            ("h:4318", Source::Base, Err(EndpointError::NotHttp)),
            ("http", Source::Base, Err(EndpointError::NotHttp)),
            ("http://", Source::Base, Err(EndpointError::NoHost)),
            ("http://:4318", Source::Base, Err(EndpointError::NoHost)),
            (
                "http://h:",
                Source::Base,
                Ok(endpoint("h", 80, "/v1/traces")),
            ),
            ("http://h:0", Source::Base, Err(EndpointError::BadPort)),
            ("http://h:65536", Source::Base, Err(EndpointError::BadPort)),
            ("http://h:+80", Source::Base, Err(EndpointError::BadPort)),
            ("http://h:80:81", Source::Base, Err(EndpointError::BadPort)),
            ("http://h x:80", Source::Base, Err(EndpointError::Extras)),
            ("http://h%2fx:80", Source::Base, Err(EndpointError::BadHost)),
            (
                "http://OTEL.Local:4318",
                Source::Base,
                Ok(endpoint("otel.local", 4318, "/v1/traces")),
            ),
            (
                "http://evil.test\\@good.example:4318",
                Source::Base,
                Ok(endpoint("evil.test", 80, "/@good.example:4318/v1/traces")),
            ),
            ("http://[::1", Source::Base, Err(EndpointError::BadHost)),
            (
                "http://[nope]:80",
                Source::Base,
                Err(EndpointError::BadHost),
            ),
            ("http://u:p@h:80", Source::Base, Err(EndpointError::Extras)),
            (
                "http://h:80/p?q=1",
                Source::Base,
                Err(EndpointError::Extras),
            ),
            ("http://h:80/p#f", Source::Base, Err(EndpointError::Extras)),
            ("http://h :80", Source::Base, Err(EndpointError::Extras)),
        ];
        for (url, source, want) in rows {
            assert_eq!(parse_endpoint(url, source), want, "{url}");
        }
    }

    #[test]
    fn the_host_header_brackets_a_v6_literal() {
        assert_eq!(endpoint("::1", 4318, "/").host_header(), "[::1]:4318");
        assert_eq!(endpoint("otel", 4318, "/").host_header(), "otel:4318");
    }

    fn planned(vars: &[(&str, &str)]) -> Plan {
        plan("intentd", |key| {
            vars.iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_owned())
        })
    }

    fn on(host: &str, path: &str, service: &str) -> Plan {
        Plan::On(Settings {
            endpoint: endpoint(host, 4318, path),
            service: service.into(),
        })
    }

    #[test]
    fn the_environment_decides_the_plan() {
        const BASE: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";
        const TRACES: &str = "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT";
        let rows: [(&[(&str, &str)], Plan); 10] = [
            (&[], Plan::Off),
            (&[(BASE, "  ")], Plan::Off),
            (&[(BASE, "http://a:4318")], on("a", "/v1/traces", "intentd")),
            (
                &[(BASE, "http://a:4318"), ("OTEL_SERVICE_NAME", "desk")],
                on("a", "/v1/traces", "desk"),
            ),
            (
                &[(BASE, "http://a:4318"), ("OTEL_SERVICE_NAME", " ")],
                on("a", "/v1/traces", "intentd"),
            ),
            (
                &[(BASE, "http://a:4318"), (TRACES, "http://b:4318/t")],
                on("b", "/t", "intentd"),
            ),
            (
                &[(BASE, "http://a:4318"), (TRACES, " ")],
                on("a", "/v1/traces", "intentd"),
            ),
            (
                &[(BASE, "http://a:4318"), ("OTEL_SDK_DISABLED", "TRUE")],
                Plan::Off,
            ),
            (
                &[(BASE, "http://a:4318"), ("OTEL_SDK_DISABLED", "false")],
                on("a", "/v1/traces", "intentd"),
            ),
            (
                &[(BASE, "https://a:4318")],
                Plan::Refused(EndpointError::NotHttp),
            ),
        ];
        for (vars, want) in rows {
            assert_eq!(planned(vars), want, "{vars:?}");
        }
    }
}
