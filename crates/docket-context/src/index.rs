//! The shadow index of one app (call lifecycle §4.6): `Reset(e)` starts an epoch, pushes in
//! that epoch complete it, a push in another epoch is refused with a request to reset, and a
//! stale signal or an app restart forgets everything.

use docket_core::IndexState;

/// What happens to an app's index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexEvent {
    /// The app starts an epoch.
    Reset(u64),
    /// The app pushes a batch in an epoch.
    Push(u64),
    /// The app signals `IndexStale`.
    Stale,
    /// The app restarted.
    AppRestarted,
}

/// What the router does with the event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexAction {
    /// Take the batch.
    Accept,
    /// Refuse the batch and ask the app for a `Reset`.
    RefuseAskReset,
    /// Nothing to do.
    None,
}

/// One transition. Total and pure.
pub fn index_step(state: IndexState, event: IndexEvent) -> (IndexState, IndexAction) {
    use IndexAction as A;
    use IndexEvent as V;
    use IndexState as S;
    match (state, event) {
        (_, V::Reset(epoch)) => (S::Syncing(epoch), A::None),
        (_, V::Stale | V::AppRestarted) => (S::Unknown, A::None),
        (S::Syncing(e), V::Push(p)) if e == p => (S::Synced(e), A::Accept),
        (S::Synced(e), V::Push(p)) if e == p => (S::Synced(e), A::Accept),
        (s, V::Push(_)) => (s, A::RefuseAskReset),
    }
}
