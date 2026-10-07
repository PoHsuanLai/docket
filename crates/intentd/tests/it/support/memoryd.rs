//! A fake memoryd on a private bus: the real wire (`almanac-dbus`'s codec and served objects)
//! over a handler the test owns. It keeps what it is asked, admits records by the rules memoryd
//! applies to the router's (`almanac-service::auth` and `docs::record_fault`, copied here:
//! the router may record anything but a `Memory` body, a message must be sent by its recording
//! actor, an episode's skeleton must be trusted and in the record's Space, and a record's label
//! must cover its documents'), and answers every caller as `Caller::Router`.
#![allow(dead_code)]

use almanac_core::{EventBody, EventRef, MemoryReply, MemoryRequest, Record, ReplicaId, Seq};
use almanac_dbus::{Call, MemoryError, Serve, decode_request, encode_reply, serve_on};
use porter_core::Count;
use prov::{Integrity, Label, SenderCheck};
use std::future::Future;
use std::os::fd::OwnedFd;
use std::sync::{Arc, Mutex};

type Handler = Box<dyn Fn(&MemoryRequest) -> Option<MemoryReply> + Send + Sync>;

/// What the fake does.
pub struct FakeMemoryd {
    /// Every request, in order.
    pub seen: Mutex<Vec<MemoryRequest>>,
    /// Every record admitted, in order.
    pub stored: Mutex<Vec<Record>>,
    /// Answers a request itself, or leaves it to the default (record what is admitted).
    handler: Handler,
}

impl std::fmt::Debug for FakeMemoryd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FakeMemoryd")
    }
}

fn label_covers(outer: &Label, inner: &Label) -> bool {
    outer.join(inner) == outer.join(outer)
}

/// Why memoryd would refuse this record from the router, or `None`.
pub fn record_fault(record: &Record) -> Option<String> {
    if matches!(record.body, EventBody::Memory { .. }) {
        return Some("the router never records a Memory body".into());
    }
    let body_fault = match &record.body {
        EventBody::Message(m) => m
            .check()
            .err()
            .map(|f| format!("message: {f:?}"))
            .or_else(|| {
                (m.sender_matches(&record.actor) == SenderCheck::Mismatch)
                    .then(|| "message: sender is not the recording actor".to_owned())
            }),
        EventBody::Episode(e) => (e.skeleton.label.integrity != Integrity::Trusted)
            .then(|| "episode: the skeleton must be trusted".to_owned())
            .or_else(|| (e.space != record.space).then(|| "episode: wrong Space".to_owned())),
        _ => None,
    };
    body_fault.or_else(|| {
        record
            .body
            .index_texts()
            .into_iter()
            .map(|t| t.label)
            .reduce(|a, b| a.join(&b))
            .filter(|joined| !label_covers(&record.label, joined))
            .map(|_| "label is less restrictive than the documents it carries".to_owned())
    })
}

impl FakeMemoryd {
    /// A memoryd that records what it admits and answers nothing else.
    pub fn recording() -> Arc<Self> {
        Self::with(|_| None)
    }

    /// A memoryd whose `handler` may answer first; `None` leaves the request to the default.
    pub fn with(
        handler: impl Fn(&MemoryRequest) -> Option<MemoryReply> + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::default(),
            stored: Mutex::default(),
            handler: Box::new(handler),
        })
    }

    /// Serves it on `connection` under the real name.
    pub async fn serve(self: &Arc<Self>, connection: &zbus::Connection) {
        serve_on(connection, self.clone())
            .await
            .expect("serve memoryd");
    }

    pub fn stored(&self) -> Vec<Record> {
        self.stored.lock().expect("lock").clone()
    }

    pub fn seen(&self) -> Vec<MemoryRequest> {
        self.seen.lock().expect("lock").clone()
    }

    fn event(&self) -> EventRef {
        let n = self.stored.lock().expect("lock").len() as u64;
        EventRef {
            space: prov::SpaceId::desktop(),
            replica: ReplicaId([7; 16]),
            seq: Seq(n),
        }
    }

    fn admit(&self, records: &[Record], batch: bool) -> MemoryReply {
        if let Some(why) = records.iter().find_map(record_fault) {
            return MemoryReply::Refused(almanac_core::Refusal::Invalid(why));
        }
        let mut stored = self.stored.lock().expect("lock");
        stored.extend(records.iter().cloned());
        drop(stored);
        let first = self.event();
        match batch {
            false => MemoryReply::Recorded(first),
            true => MemoryReply::RecordedBatch(first, Count(records.len() as u32)),
        }
    }

    fn answer(&self, request: &MemoryRequest) -> MemoryReply {
        if let Some(reply) = (self.handler)(request) {
            return reply;
        }
        match request {
            MemoryRequest::Record(record) => self.admit(std::slice::from_ref(record), false),
            MemoryRequest::RecordBatch(records) => self.admit(records, true),
            _ => MemoryReply::Refused(almanac_core::Refusal::NotAllowed),
        }
    }
}

impl Serve for FakeMemoryd {
    fn serve(
        &self,
        _sender: &str,
        call: Call,
        _fd: Option<OwnedFd>,
    ) -> impl Future<Output = Result<Vec<String>, MemoryError>> + Send {
        let outcome = decode_request(&call)
            .map_err(MemoryError::from)
            .and_then(|request| {
                self.seen.lock().expect("lock").push(request.clone());
                encode_reply(&call, &self.answer(&request))
            });
        async move { outcome }
    }
}
