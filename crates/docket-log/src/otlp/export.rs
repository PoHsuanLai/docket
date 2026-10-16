//! The exporter thread: finished spans arrive on a bounded queue, a worker batches them (up to
//! 512 spans or 5 seconds from the first) and POSTs each batch. A failed batch is dropped with one
//! warning per minute at most; a full queue drops the span and counts it.

use super::config::Settings;
use super::span::Span;
use super::{SIDE, Side, encode, http};
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

/// Spans waiting for the worker.
const QUEUE: usize = 2048;
/// The most spans in one POST.
const BATCH: usize = 512;
/// How long a batch waits for more spans after its first.
const WINDOW: Duration = Duration::from_secs(5);
/// The least time between two warnings of one kind.
const WARN_EVERY: Duration = Duration::from_secs(60);

/// Spans that were not exported, by cause; read and cleared by the worker.
#[derive(Debug, Default)]
pub(crate) struct Stats {
    /// Spans dropped because the queue was full.
    pub queue_full: AtomicU64,
    /// Spans never recorded because the store of open spans was full.
    pub store_full: AtomicU64,
}

impl Stats {
    fn any(&self) -> bool {
        self.queue_full.load(Ordering::Relaxed) + self.store_full.load(Ordering::Relaxed) > 0
    }

    /// The counts, back to zero.
    fn take(&self) -> (u64, u64) {
        (
            self.queue_full.swap(0, Ordering::Relaxed),
            self.store_full.swap(0, Ordering::Relaxed),
        )
    }
}

/// Lets something through at most once in `every`.
#[derive(Debug)]
struct Limiter {
    every: Duration,
    last: Option<Instant>,
}

impl Limiter {
    fn new(every: Duration) -> Self {
        Self { every, last: None }
    }

    fn allow(&mut self, now: Instant) -> bool {
        match self.last {
            Some(last) if now.saturating_duration_since(last) < self.every => false,
            _ => {
                self.last = Some(now);
                true
            }
        }
    }
}

/// Starts the worker thread. The queue's sending end is the recorder's; when it is dropped the
/// worker ends.
pub(crate) fn spawn(settings: Settings) -> io::Result<(SyncSender<Span>, Arc<Stats>)> {
    spawn_with(settings, WINDOW)
}

fn spawn_with(settings: Settings, window: Duration) -> io::Result<(SyncSender<Span>, Arc<Stats>)> {
    let (out, queue) = mpsc::sync_channel(QUEUE);
    let stats = Arc::new(Stats::default());
    let worker = Worker {
        queue,
        settings,
        window,
        stats: Arc::clone(&stats),
    };
    thread::Builder::new()
        .name("docket-log-otlp".to_owned())
        .spawn(move || worker.run())?;
    Ok((out, stats))
}

#[derive(Debug)]
struct Worker {
    queue: Receiver<Span>,
    settings: Settings,
    window: Duration,
    stats: Arc<Stats>,
}

impl Worker {
    fn run(self) {
        SIDE.set(Side::Exporter);
        let mut failures = Limiter::new(WARN_EVERY);
        let mut drops = Limiter::new(WARN_EVERY);
        while let Some(batch) = self.next_batch() {
            let body = encode::request(&self.settings.service, &batch).to_string();
            let sent = http::post(&self.settings.endpoint, body.as_bytes());
            if let Err(why) = sent
                && failures.allow(Instant::now())
            {
                tracing::warn!(%why, spans = batch.len(), "telemetry export failed; the batch was dropped");
            }
            if self.stats.any() && drops.allow(Instant::now()) {
                let (queue_full, store_full) = self.stats.take();
                tracing::warn!(queue_full, store_full, "spans were not exported");
            }
        }
    }

    /// The next batch: waits for a span, then takes more until the batch is full or the window
    /// since the first has passed. None when the queue's sending end is gone.
    fn next_batch(&self) -> Option<Vec<Span>> {
        let mut batch = vec![self.queue.recv().ok()?];
        let deadline = Instant::now() + self.window;
        while batch.len() < BATCH {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.queue.recv_timeout(left) {
                Ok(span) => batch.push(span),
                Err(_) => break,
            }
        }
        Some(batch)
    }
}

#[cfg(test)]
mod tests {
    use super::super::config::Endpoint;
    use super::super::span::{SpanId, TraceId};
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn a_limiter_lets_one_through_per_period() {
        let start = Instant::now();
        let mut limiter = Limiter::new(Duration::from_secs(60));
        assert!(limiter.allow(start));
        assert!(!limiter.allow(start + Duration::from_secs(59)));
        assert!(limiter.allow(start + Duration::from_secs(60)));
        assert!(!limiter.allow(start + Duration::from_secs(61)));
    }

    #[test]
    fn stats_are_read_and_cleared() {
        let stats = Stats::default();
        assert!(!stats.any());
        stats.queue_full.fetch_add(2, Ordering::Relaxed);
        stats.store_full.fetch_add(1, Ordering::Relaxed);
        assert!(stats.any());
        assert_eq!(stats.take(), (2, 1));
        assert!(!stats.any());
    }

    fn span(name: &'static str) -> Span {
        Span {
            name,
            trace: TraceId(1),
            id: SpanId(2),
            parent: None,
            start: 10,
            end: 20,
            fields: Vec::new(),
        }
    }

    /// Whether `data` holds a whole request: the head and `Content-Length` bytes of body.
    fn complete(data: &[u8]) -> bool {
        let text = String::from_utf8_lossy(data);
        let Some((head, body)) = text.split_once("\r\n\r\n") else {
            return false;
        };
        let length = head
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        body.len() >= length
    }

    /// A collector for one request: waits at most ten seconds for the connection and five for
    /// each read, answers 200, and returns what it was sent.
    fn collect_one(listener: &TcpListener) -> String {
        listener.set_nonblocking(true).expect("nonblocking");
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(why)
                    if why.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(why) => panic!("the exporter never connected: {why}"),
            }
        };
        stream.set_nonblocking(false).expect("blocking");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        let (mut data, mut buf) = (Vec::new(), [0_u8; 4096]);
        while !complete(&data) {
            let read = stream.read(&mut buf).expect("read the request");
            assert!(read > 0, "the exporter closed before the request was whole");
            data.extend_from_slice(&buf[..read]);
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .expect("reply");
        String::from_utf8(data).expect("utf-8 request")
    }

    #[test]
    fn a_batch_reaches_a_loopback_collector_as_one_json_post() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("address").port();
        let collector = thread::spawn(move || collect_one(&listener));
        let settings = Settings {
            endpoint: Endpoint {
                host: "127.0.0.1".into(),
                port,
                path: "/v1/traces".into(),
            },
            service: "testd".into(),
        };
        let (out, _stats) = spawn_with(settings, Duration::from_millis(200)).expect("spawn");
        out.send(span("execute_tool")).expect("queue");
        out.send(span("invoke_agent")).expect("queue");
        let request = collector.join().expect("collector");
        let (head, body) = request.split_once("\r\n\r\n").expect("head and body");
        let head = head.to_ascii_lowercase();
        assert!(head.starts_with("post /v1/traces http/1.1\r\n"), "{head}");
        assert!(
            head.contains("\r\ncontent-type: application/json\r\n"),
            "{head}"
        );
        assert!(head.contains("\r\nconnection: close\r\n"), "{head}");
        let body: serde_json::Value = serde_json::from_str(body).expect("a JSON body");
        let resource = &body["resourceSpans"][0];
        assert_eq!(
            resource["resource"]["attributes"][0]["value"]["stringValue"],
            "testd"
        );
        let names: Vec<_> = resource["scopeSpans"][0]["spans"]
            .as_array()
            .expect("spans")
            .iter()
            .map(|span| span["name"].as_str().unwrap_or_default().to_owned())
            .collect();
        assert_eq!(names, ["execute_tool", "invoke_agent"]);
    }
}
