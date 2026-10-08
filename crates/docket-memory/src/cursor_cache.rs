//! Where almanac's cursor stood after the rows a page of a session's log ended on, so the next
//! page resumes there. Only the newest stop of each session is kept: paging goes forward, and a
//! read from an older position just counts from the first row.

use almanac_core::Cursor;
use docket_session::Seq;
use prov::SessionId;
use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(Debug, Default)]
pub(crate) struct CursorCache(Mutex<BTreeMap<SessionId, (Seq, Cursor)>>);

impl CursorCache {
    /// The cursor of `session` at exactly `place`, if that is where it last stopped.
    pub(crate) fn at(&self, session: &SessionId, place: Seq) -> Option<Cursor> {
        let all = self.0.lock().ok()?;
        all.get(session)
            .filter(|(stop, _)| *stop == place)
            .map(|(_, cursor)| *cursor)
    }

    /// Notes that `session`'s read stopped at `place` with `cursor`, replacing the older stop.
    pub(crate) fn stop(&self, session: &SessionId, place: Seq, cursor: Cursor) {
        if let Ok(mut all) = self.0.lock() {
            all.insert(session.clone(), (place, cursor));
        }
    }

    /// Drops what is known of `session`.
    pub(crate) fn forget(&self, session: &SessionId) {
        if let Ok(mut all) = self.0.lock() {
            all.remove(session);
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.0.lock().map_or(0, |all| all.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str) -> SessionId {
        SessionId::parse(id).expect("session")
    }

    #[test]
    fn only_the_newest_stop_of_a_session_is_kept() {
        let cache = CursorCache::default();
        let id = session("s-1");
        for n in 1..=50 {
            cache.stop(&id, Seq(n), Cursor(almanac_core::Seq(n)));
        }
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.at(&id, Seq(50)), Some(Cursor(almanac_core::Seq(50))));
        assert_eq!(cache.at(&id, Seq(49)), None);
        assert_eq!(cache.at(&session("s-2"), Seq(50)), None);
    }

    #[test]
    fn forgetting_a_session_drops_its_stop() {
        let cache = CursorCache::default();
        let id = session("s-1");
        cache.stop(&id, Seq(3), Cursor(almanac_core::Seq(3)));
        cache.forget(&id);
        assert_eq!(cache.len(), 0);
    }
}
