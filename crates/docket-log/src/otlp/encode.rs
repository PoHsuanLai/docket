//! The OTLP/HTTP JSON body: an `ExportTraceServiceRequest` of resource spans, scope spans, spans.
//! Ids are lowercase hex (32 and 16 digits), times are unix-nanosecond decimal strings, and an
//! `intValue` is a decimal string, as the OTLP JSON mapping says.

use super::span::{Span, Value};
use serde_json::{Value as Json, json};

/// `SPAN_KIND_INTERNAL`: the router's spans are in-process work.
const KIND_INTERNAL: u8 = 1;

/// The request body for `spans`, sent by the service `service`.
pub(crate) fn request(service: &str, spans: &[Span]) -> Json {
    json!({
        "resourceSpans": [{
            "resource": { "attributes": [attribute("service.name", &Value::Text(service.to_owned()))] },
            "scopeSpans": [{
                "scope": { "name": "docket-log", "version": env!("CARGO_PKG_VERSION") },
                "spans": spans.iter().map(span).collect::<Vec<_>>(),
            }],
        }],
    })
}

fn span(span: &Span) -> Json {
    let mut out = json!({
        "traceId": span.trace.hex(),
        "spanId": span.id.hex(),
        "name": span.name,
        "kind": KIND_INTERNAL,
        "startTimeUnixNano": span.start.to_string(),
        "endTimeUnixNano": span.end.to_string(),
        "attributes": span.fields.iter().map(|(key, value)| attribute(key, value)).collect::<Vec<_>>(),
    });
    if let (Some(parent), Some(map)) = (span.parent, out.as_object_mut()) {
        map.insert("parentSpanId".to_owned(), Json::String(parent.hex()));
    }
    out
}

fn attribute(key: &str, value: &Value) -> Json {
    json!({ "key": key, "value": any_value(value) })
}

/// An `AnyValue`. An integer past `i64` and a non-finite float (which JSON numbers cannot hold)
/// go out as strings.
fn any_value(value: &Value) -> Json {
    match value {
        Value::Text(text) => json!({ "stringValue": text }),
        Value::Signed(number) => json!({ "intValue": number.to_string() }),
        Value::Unsigned(number) => match i64::try_from(*number) {
            Ok(fits) => json!({ "intValue": fits.to_string() }),
            Err(_) => json!({ "stringValue": number.to_string() }),
        },
        Value::Flag(flag) => json!({ "boolValue": flag }),
        Value::Real(number) if number.is_finite() => json!({ "doubleValue": number }),
        Value::Real(number) => json!({ "stringValue": number.to_string() }),
    }
}

#[cfg(test)]
mod tests {
    use super::super::span::{SpanId, TraceId};
    use super::*;

    fn sample(parent: Option<SpanId>, fields: Vec<(&'static str, Value)>) -> Span {
        Span {
            name: "execute_tool",
            trace: TraceId(0x0102_0304_0506_0708_090a_0b0c_0d0e_0f10),
            id: SpanId(0x00ab),
            parent,
            start: 1_700_000_000_000_000_001,
            end: 1_700_000_000_000_000_999,
            fields,
        }
    }

    #[test]
    fn a_root_span_has_each_attribute_type_and_no_parent() {
        let fields = vec![
            ("text", Value::Text("a".into())),
            ("signed", Value::Signed(-3)),
            ("unsigned", Value::Unsigned(7)),
            ("big", Value::Unsigned(u64::MAX)),
            ("flag", Value::Flag(true)),
            ("real", Value::Real(1.5)),
            ("nan", Value::Real(f64::NAN)),
        ];
        let body = request("intentd", &[sample(None, fields)]);
        let want = json!({
            "resourceSpans": [{
                "resource": { "attributes": [
                    { "key": "service.name", "value": { "stringValue": "intentd" } }
                ] },
                "scopeSpans": [{
                    "scope": { "name": "docket-log", "version": env!("CARGO_PKG_VERSION") },
                    "spans": [{
                        "traceId": "0102030405060708090a0b0c0d0e0f10",
                        "spanId": "00000000000000ab",
                        "name": "execute_tool",
                        "kind": 1,
                        "startTimeUnixNano": "1700000000000000001",
                        "endTimeUnixNano": "1700000000000000999",
                        "attributes": [
                            { "key": "text", "value": { "stringValue": "a" } },
                            { "key": "signed", "value": { "intValue": "-3" } },
                            { "key": "unsigned", "value": { "intValue": "7" } },
                            { "key": "big", "value": { "stringValue": "18446744073709551615" } },
                            { "key": "flag", "value": { "boolValue": true } },
                            { "key": "real", "value": { "doubleValue": 1.5 } },
                            { "key": "nan", "value": { "stringValue": "NaN" } },
                        ],
                    }],
                }],
            }],
        });
        assert_eq!(body, want);
    }

    #[test]
    fn a_child_span_names_its_parent_in_hex() {
        let body = request("d", &[sample(Some(SpanId(0xcafe)), Vec::new())]);
        let spans = &body["resourceSpans"][0]["scopeSpans"][0]["spans"];
        assert_eq!(spans[0]["parentSpanId"], "000000000000cafe");
        assert_eq!(spans[0]["attributes"], json!([]));
    }
}
