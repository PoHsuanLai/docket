//! A finished span as the exporter holds it, the visitor that reads a span's fields, and the
//! ids. Ids need to be unique, not secret: they are a per-process random key hashed with a
//! counter, never a source of anything that must be unguessable.

use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::field::{Field, Visit};

/// A 128-bit trace id; never zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TraceId(pub u128);

/// A 64-bit span id; never zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SpanId(pub u64);

impl TraceId {
    /// 32 lowercase hex digits.
    pub(crate) fn hex(self) -> String {
        format!("{:032x}", self.0)
    }
}

impl SpanId {
    /// 16 lowercase hex digits.
    pub(crate) fn hex(self) -> String {
        format!("{:016x}", self.0)
    }
}

/// A span field's value, as the visitor met it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Text(String),
    Signed(i64),
    Unsigned(u64),
    Flag(bool),
    Real(f64),
}

/// The fields of a span, in the order they were first set.
pub(crate) type Fields = Vec<(&'static str, Value)>;

/// A span that has ended (or is still open, with `end` zero).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Span {
    pub name: &'static str,
    pub trace: TraceId,
    pub id: SpanId,
    pub parent: Option<SpanId>,
    /// Unix nanoseconds.
    pub start: u64,
    /// Unix nanoseconds; zero while the span is open.
    pub end: u64,
    pub fields: Fields,
}

/// Sets `name` in `fields`, replacing an earlier value of the same name.
pub(crate) fn put(fields: &mut Fields, name: &'static str, value: Value) {
    match fields.iter_mut().find(|(known, _)| *known == name) {
        Some(slot) => slot.1 = value,
        None => fields.push((name, value)),
    }
}

/// Reads the fields a span declares and sets. Anything that is not a plain number, flag or
/// string is kept as its `Debug` text (what `%display` and `?debug` fields arrive as).
#[derive(Debug, Default)]
pub(crate) struct Collect(pub Fields);

impl Visit for Collect {
    fn record_str(&mut self, field: &Field, value: &str) {
        put(&mut self.0, field.name(), Value::Text(value.to_owned()));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        put(&mut self.0, field.name(), Value::Signed(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        put(&mut self.0, field.name(), Value::Unsigned(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        put(&mut self.0, field.name(), Value::Flag(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        put(&mut self.0, field.name(), Value::Real(value));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        put(&mut self.0, field.name(), Value::Text(format!("{value:?}")));
    }
}

/// Unix nanoseconds now; zero if the clock is before the epoch.
pub(crate) fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since| u64::try_from(since.as_nanos()).ok())
        .unwrap_or(0)
}

/// The id source: the standard library's per-process random hasher keys, mixed with a counter.
#[derive(Debug)]
pub(crate) struct Ids {
    state: RandomState,
    counter: AtomicU64,
}

impl Ids {
    pub(crate) fn new() -> Self {
        Self {
            state: RandomState::new(),
            counter: AtomicU64::new(0),
        }
    }

    /// The next 64 bits; never zero.
    fn next(&self) -> u64 {
        let mut hasher = self.state.build_hasher();
        hasher.write_u64(self.counter.fetch_add(1, Ordering::Relaxed));
        hasher.finish().max(1)
    }

    pub(crate) fn span(&self) -> SpanId {
        SpanId(self.next())
    }

    pub(crate) fn trace(&self) -> TraceId {
        TraceId((u128::from(self.next()) << 64) | u128::from(self.next()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_wide_hex_nonzero_and_distinct() {
        let ids = Ids::new();
        let spans: HashSet<u64> = (0..1000).map(|_| ids.span().0).collect();
        let traces: HashSet<u128> = (0..1000).map(|_| ids.trace().0).collect();
        assert_eq!(spans.len(), 1000);
        assert_eq!(traces.len(), 1000);
        assert!(!spans.contains(&0) && !traces.contains(&0));
        assert_eq!(SpanId(0xab).hex(), "00000000000000ab");
        assert_eq!(TraceId(0xab).hex(), format!("{:0>30}ab", ""));
    }

    #[test]
    fn a_field_set_twice_keeps_its_place_and_its_last_value() {
        let mut fields = Fields::new();
        put(&mut fields, "a", Value::Signed(1));
        put(&mut fields, "b", Value::Flag(true));
        put(&mut fields, "a", Value::Signed(2));
        assert_eq!(fields, [("a", Value::Signed(2)), ("b", Value::Flag(true))]);
    }
}
