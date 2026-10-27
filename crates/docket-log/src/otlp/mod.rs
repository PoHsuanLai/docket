//! Span export over OTLP/HTTP JSON, behind the `otlp` feature. `init` reads the environment once
//! (`config`); when export is on, the subscriber also records spans (`store`), and a worker thread
//! batches the finished ones and POSTs them to a collector (`export`, `http`, `encode`).
//!
//! Only `http://` endpoints are accepted: a person who wants TLS or a remote backend runs a local
//! collector. A span is exported as the fields it declared and nothing else (no events, no
//! messages); callers must never put content on a span.

mod config;
mod encode;
mod export;
mod http;
mod span;
mod store;

use config::Plan;
use export::Stats;
use span::Span;
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::mpsc::{SyncSender, TrySendError};
use store::{Closed, Store};
use tracing::span::{Attributes, Id, Record};

/// The most open spans kept at once; past it new spans are not recorded and are counted.
const MAX_OPEN: usize = 4096;

/// Which thread is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    /// The exporter's own thread: spans made here are never recorded, so the exporter cannot
    /// export itself.
    Exporter,
    Other,
}

thread_local! {
    static SIDE: Cell<Side> = const { Cell::new(Side::Other) };
    /// The spans this thread has entered, innermost last: what a contextual span's parent is.
    static ENTERED: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

/// The subscriber's span recording: the open spans, and the queue finished ones go to.
#[derive(Debug)]
pub(crate) struct Recorder {
    store: Store,
    out: SyncSender<Span>,
    stats: Arc<Stats>,
}

impl Recorder {
    fn new(out: SyncSender<Span>, stats: Arc<Stats>, cap: usize) -> Self {
        Self {
            store: Store::new(cap, Arc::clone(&stats)),
            out,
            stats,
        }
    }

    pub(crate) fn new_span(&self, attrs: &Attributes<'_>, id: &Id) {
        if SIDE.get() == Side::Exporter {
            return;
        }
        let parent = if attrs.is_root() {
            None
        } else if let Some(parent) = attrs.parent() {
            Some(parent.into_u64())
        } else {
            ENTERED.with_borrow(|entered| entered.last().copied())
        };
        self.store.open(id.into_u64(), attrs, parent);
    }

    pub(crate) fn record(&self, id: &Id, values: &Record<'_>) {
        self.store.record(id.into_u64(), values);
    }

    pub(crate) fn clone_span(&self, id: &Id) {
        self.store.hold(id.into_u64());
    }

    /// One handle to the span is gone. True when no handle remains. The span is queued for export
    /// by the last one, so it ends exactly once however often it was cloned.
    pub(crate) fn try_close(&self, id: &Id) -> bool {
        match self.store.close(id.into_u64()) {
            Closed::Held => false,
            Closed::Untracked => true,
            Closed::Ended(span) => {
                if let Err(TrySendError::Full(_)) = self.out.try_send(span) {
                    self.stats
                        .queue_full
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                true
            }
        }
    }

    pub(crate) fn enter(&self, id: &Id) {
        ENTERED.with_borrow_mut(|entered| entered.push(id.into_u64()));
    }

    pub(crate) fn exit(&self, id: &Id) {
        let id = id.into_u64();
        ENTERED.with_borrow_mut(|entered| {
            if let Some(at) = entered.iter().rposition(|open| *open == id) {
                entered.remove(at);
            }
        });
    }
}

/// Reads the environment and, when export is on, starts the worker. The recorder is none when
/// export is off or could not start; the text, when there is one, says why a requested export is
/// off (the caller logs it once the subscriber is installed).
pub(crate) fn start(daemon: &str) -> (Option<Recorder>, Option<String>) {
    start_from(daemon, |key| std::env::var(key).ok())
}

fn start_from(
    daemon: &str,
    get: impl Fn(&str) -> Option<String>,
) -> (Option<Recorder>, Option<String>) {
    match config::plan(daemon, get) {
        Plan::Off => (None, None),
        Plan::Refused(why) => (None, Some(why.to_string())),
        Plan::On(settings) => match export::spawn(settings) {
            Ok((out, stats)) => (Some(Recorder::new(out, stats, MAX_OPEN)), None),
            Err(why) => (
                None,
                Some(format!("the exporter thread did not start: {why}")),
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::span::Value;
    use super::*;
    use crate::{Lines, Reader};
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc::{self, Receiver};

    /// A subscriber that records into a channel the test reads.
    fn recording(cap: usize) -> (Arc<Lines>, Receiver<Span>, Arc<Stats>) {
        let (out, finished) = mpsc::sync_channel(16);
        let stats = Arc::new(Stats::default());
        let lines = Lines {
            daemon: "testd",
            reader: Reader::Plain,
            spans: AtomicU64::new(0),
            recorder: Some(Recorder::new(out, Arc::clone(&stats), cap)),
        };
        (Arc::new(lines), finished, stats)
    }

    fn open_spans(lines: &Lines) -> usize {
        lines
            .recorder
            .as_ref()
            .map_or(0, |recorder| recorder.store.len())
    }

    fn names(spans: &[Span]) -> Vec<&'static str> {
        spans.iter().map(|span| span.name).collect()
    }

    #[test]
    fn a_span_ends_once_however_often_it_was_cloned_and_children_inherit_the_trace() {
        let (lines, finished, _) = recording(16);
        tracing::subscriber::with_default(Arc::clone(&lines), || {
            let root = tracing::info_span!("root", a = 1_u64, late = tracing::field::Empty);
            let child = tracing::info_span!(parent: &root, "child", who = "x");
            root.record("late", 5_i64);
            let copy = root.clone();
            drop(child);
            drop(root);
            let early: Vec<_> = finished.try_iter().collect();
            assert_eq!(names(&early), ["child"], "root still has a handle");
            drop(copy);
            let late: Vec<_> = finished.try_iter().collect();
            assert_eq!(names(&late), ["root"], "root ended once");
            let (child, root) = (&early[0], &late[0]);
            assert_eq!(child.trace, root.trace);
            assert_eq!(child.parent, Some(root.id));
            assert_eq!(root.parent, None);
            assert_eq!(
                root.fields,
                [("a", Value::Unsigned(1)), ("late", Value::Signed(5))]
            );
            assert_eq!(child.fields, [("who", Value::Text("x".into()))]);
            assert!(root.start <= root.end && root.start > 0);
        });
        assert_eq!(open_spans(&lines), 0);
        assert_eq!(finished.try_iter().count(), 0, "nothing ended twice");
    }

    #[test]
    fn a_span_made_while_another_is_entered_is_its_child_and_a_later_one_is_a_new_trace() {
        let (lines, finished, _) = recording(16);
        tracing::subscriber::with_default(lines, || {
            let outer = tracing::info_span!("outer");
            let inner = outer.in_scope(|| tracing::info_span!("inner"));
            let after = tracing::info_span!("after");
            drop((inner, after, outer));
        });
        let all: Vec<_> = finished.try_iter().collect();
        let by_name = |name: &str| all.iter().find(|span| span.name == name).expect("span");
        let (outer, inner, after) = (by_name("outer"), by_name("inner"), by_name("after"));
        assert_eq!(inner.parent, Some(outer.id));
        assert_eq!(inner.trace, outer.trace);
        assert_eq!(after.parent, None);
        assert_ne!(after.trace, outer.trace);
    }

    #[test]
    fn past_the_cap_new_spans_are_counted_and_not_recorded() {
        let (lines, finished, stats) = recording(2);
        tracing::subscriber::with_default(Arc::clone(&lines), || {
            let kept = (tracing::info_span!("a"), tracing::info_span!("b"));
            let refused = tracing::info_span!("c");
            assert_eq!(stats.store_full.load(Ordering::Relaxed), 1);
            assert_eq!(open_spans(&lines), 2);
            drop(refused);
            assert_eq!(
                finished.try_iter().count(),
                0,
                "an unrecorded span exports nothing"
            );
            drop(kept);
        });
        assert_eq!(finished.try_iter().count(), 2);
        assert_eq!(open_spans(&lines), 0);
    }

    #[test]
    fn a_full_queue_drops_the_span_and_counts_it() {
        let (out, finished) = mpsc::sync_channel(1);
        let stats = Arc::new(Stats::default());
        let lines = Lines {
            daemon: "testd",
            reader: Reader::Plain,
            spans: AtomicU64::new(0),
            recorder: Some(Recorder::new(out, Arc::clone(&stats), 16)),
        };
        tracing::subscriber::with_default(lines, || {
            drop(tracing::info_span!("one"));
            drop(tracing::info_span!("two"));
        });
        assert_eq!(finished.try_iter().count(), 1);
        assert_eq!(stats.queue_full.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn spans_made_on_the_exporter_side_are_not_recorded() {
        let (lines, finished, _) = recording(16);
        tracing::subscriber::with_default(Arc::clone(&lines), || {
            SIDE.set(Side::Exporter);
            drop(tracing::info_span!("own"));
            SIDE.set(Side::Other);
        });
        assert_eq!(finished.try_iter().count(), 0);
        assert_eq!(open_spans(&lines), 0);
    }

    #[test]
    fn a_span_is_exported_by_its_fields_only_never_by_what_is_logged_inside_it() {
        let (lines, finished, _) = recording(16);
        tracing::subscriber::with_default(lines, || {
            let span = tracing::info_span!("work", prompt = "hello", n = 2_u64);
            let _inside = span.enter();
            tracing::info!(secret = "event text", "event message");
        });
        let spans: Vec<_> = finished.try_iter().collect();
        assert_eq!(
            spans[0].fields,
            [
                ("prompt", Value::Text("hello".into())),
                ("n", Value::Unsigned(2))
            ]
        );
        let body = encode::request("testd", &spans);
        let attributes = &body["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["attributes"];
        assert_eq!(
            *attributes,
            json!([
                { "key": "prompt", "value": { "stringValue": "hello" } },
                { "key": "n", "value": { "intValue": "2" } },
            ])
        );
        let text = body.to_string();
        assert!(!text.contains("event text") && !text.contains("event message"));
    }

    #[test]
    fn start_follows_the_plan() {
        let env = |pairs: &'static [(&str, &str)]| {
            move |key: &str| {
                pairs
                    .iter()
                    .find(|(name, _)| *name == key)
                    .map(|(_, value)| (*value).to_owned())
            }
        };
        let (recorder, note) = start_from("testd", env(&[]));
        assert!(recorder.is_none() && note.is_none());
        let (recorder, note) = start_from(
            "testd",
            env(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "https://collector:4318")]),
        );
        assert!(recorder.is_none());
        assert!(note.is_some_and(|text| text.contains("http://")));
        let (recorder, note) = start_from(
            "testd",
            env(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "http://127.0.0.1:4318")]),
        );
        assert!(recorder.is_some() && note.is_none());
    }
}
