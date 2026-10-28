//! A watch on the inferd link: the seam a trace hooks into. The link tells its watch about every
//! frame sent and every event received, and does nothing else with them. What a watch keeps, and
//! where, is the watcher's (the test-only `docket-trace` crate keeps the words of each exchange);
//! this crate holds no code that writes a prompt anywhere, and a link with no watch takes no copy
//! of anything.

use porter_capability::Tier;
use porter_client::{Transport, TransportError};
use porter_core::DataClass;
use porter_infer::{ClientFrame, InferEvent, InferSession, OpenOptions, Readiness, SessionError};
use porter_match::Need;
use porter_wire::{AccountsReply, AccountsRequest};
use std::fmt::Debug;
use std::sync::Arc;

/// Whoever is told what crosses the link. It makes one [`SessionWatch`] for each session opened.
pub trait InferObserver: Debug + Send + Sync {
    /// The watch for a session that was just opened.
    fn session(&self) -> Box<dyn SessionWatch>;
}

/// What one session's watcher is told, in order.
pub trait SessionWatch: Debug + Send {
    /// A frame is about to be sent.
    fn sent(&mut self, frame: &ClientFrame);
    /// An event arrived.
    fn received(&mut self, event: &InferEvent);
}

/// The watch a daemon is given: an observer, or none.
#[derive(Debug, Clone, Default)]
pub struct InferWatch(Option<Arc<dyn InferObserver>>);

impl InferWatch {
    /// A watch that sees nothing.
    pub fn off() -> Self {
        Self(None)
    }

    /// A watch that tells `observer` about every session.
    pub fn new(observer: Arc<dyn InferObserver>) -> Self {
        Self(Some(observer))
    }

    fn session(&self) -> Option<Box<dyn SessionWatch>> {
        self.0.as_ref().map(|observer| observer.session())
    }
}

/// A transport that tells its watch about every session and otherwise is `T`.
#[derive(Debug)]
pub struct Watched<T> {
    inner: T,
    watch: InferWatch,
}

impl<T> Watched<T> {
    /// `inner`, with `watch` on its sessions.
    pub fn new(inner: T, watch: InferWatch) -> Self {
        Self { inner, watch }
    }

    /// The transport underneath.
    pub fn inner(&self) -> &T {
        &self.inner
    }
}

/// A session that tells its watch about each frame and event.
#[derive(Debug)]
pub struct WatchedSession<S> {
    inner: S,
    watch: Option<Box<dyn SessionWatch>>,
}

impl<S: InferSession> InferSession for WatchedSession<S> {
    async fn send(&mut self, frame: ClientFrame) -> Result<(), SessionError> {
        if let Some(watch) = self.watch.as_mut() {
            watch.sent(&frame);
        }
        self.inner.send(frame).await
    }

    async fn send_attached(
        &mut self,
        frame: ClientFrame,
        attachments: Vec<std::os::fd::OwnedFd>,
    ) -> Result<(), SessionError> {
        if let Some(watch) = self.watch.as_mut() {
            watch.sent(&frame);
        }
        self.inner.send_attached(frame, attachments).await
    }

    async fn next(&mut self) -> Result<InferEvent, SessionError> {
        let event = self.inner.next().await?;
        if let Some(watch) = self.watch.as_mut() {
            watch.received(&event);
        }
        Ok(event)
    }
}

impl<T: Transport> Transport for Watched<T> {
    type Session = WatchedSession<T::Session>;

    async fn call(&self, request: AccountsRequest) -> Result<AccountsReply, TransportError> {
        self.inner.call(request).await
    }

    async fn open_with(
        &self,
        need: &Need,
        class: DataClass,
        tier: Tier,
        options: &OpenOptions,
    ) -> Result<Self::Session, TransportError> {
        let inner = self.inner.open_with(need, class, tier, options).await?;
        Ok(WatchedSession {
            inner,
            watch: self.watch.session(),
        })
    }

    async fn prepare(
        &self,
        need: &Need,
        class: DataClass,
        tier: Tier,
        options: &OpenOptions,
    ) -> Result<Readiness, TransportError> {
        self.inner.prepare(need, class, tier, options).await
    }
}
