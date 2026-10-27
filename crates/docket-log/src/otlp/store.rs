//! The spans that are open: kept from `new_span` to the last `try_close`, bounded.

use super::export::Stats;
use super::span::{Collect, Ids, Span, now_nanos, put};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tracing::span::{Attributes, Record};

#[derive(Debug)]
struct Live {
    span: Span,
    /// The handles to the span: one from `new_span`, one more for each `clone_span`.
    refs: usize,
}

/// What closing one handle did.
#[derive(Debug, PartialEq)]
pub(crate) enum Closed {
    /// The span was never kept (the store was full): nothing to do.
    Untracked,
    /// Other handles remain.
    Held,
    /// That was the last handle: the span has ended, once.
    Ended(Span),
}

/// The open spans, by the id `tracing` knows them by.
#[derive(Debug)]
pub(crate) struct Store {
    live: Mutex<HashMap<u64, Live>>,
    ids: Ids,
    cap: usize,
    stats: Arc<Stats>,
}

impl Store {
    /// A store that keeps at most `cap` open spans; past that, new spans are counted in `stats`
    /// and not kept.
    pub(crate) fn new(cap: usize, stats: Arc<Stats>) -> Self {
        Self {
            live: Mutex::new(HashMap::new()),
            ids: Ids::new(),
            cap,
            stats,
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<u64, Live>> {
        self.live.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts keeping a span. `parent` is the `tracing` id of its parent: a kept parent gives the
    /// trace id and the parent span id, anything else makes the span a root with a new trace.
    pub(crate) fn open(&self, id: u64, attrs: &Attributes<'_>, parent: Option<u64>) {
        let mut fields = Collect::default();
        attrs.record(&mut fields);
        let mut live = self.lock();
        if live.len() >= self.cap {
            self.stats.store_full.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let (trace, parent) = match parent.and_then(|parent| live.get(&parent)) {
            Some(parent) => (parent.span.trace, Some(parent.span.id)),
            None => (self.ids.trace(), None),
        };
        let span = Span {
            name: attrs.metadata().name(),
            trace,
            id: self.ids.span(),
            parent,
            start: now_nanos(),
            end: 0,
            fields: fields.0,
        };
        live.insert(id, Live { span, refs: 1 });
    }

    /// Adds fields recorded after the span started.
    pub(crate) fn record(&self, id: u64, values: &Record<'_>) {
        let mut fields = Collect::default();
        values.record(&mut fields);
        if let Some(live) = self.lock().get_mut(&id) {
            for (name, value) in fields.0 {
                put(&mut live.span.fields, name, value);
            }
        }
    }

    /// One more handle to the span.
    pub(crate) fn hold(&self, id: u64) {
        if let Some(live) = self.lock().get_mut(&id) {
            live.refs += 1;
        }
    }

    /// One handle fewer; the last one ends the span and takes it out of the store.
    pub(crate) fn close(&self, id: u64) -> Closed {
        let mut live = self.lock();
        let Some(open) = live.get_mut(&id) else {
            return Closed::Untracked;
        };
        open.refs = open.refs.saturating_sub(1);
        if open.refs > 0 {
            return Closed::Held;
        }
        match live.remove(&id) {
            Some(Live { mut span, .. }) => {
                span.end = now_nanos();
                Closed::Ended(span)
            }
            None => Closed::Untracked,
        }
    }

    /// How many spans are open.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lock().len()
    }
}
