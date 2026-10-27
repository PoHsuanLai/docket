//! What the routines' scheduler needs from its surroundings, as one seam: the local zone, the
//! helpers' Spaces and names, posting a job, how a job ended, the stored records, the sentences the
//! person is told, the almanac's events, the systemd timers and keeping the desktop awake. The
//! daemon passes the hub's; a test passes a fake that records what it was asked.

use companion_core::{Awake, EventAsk, Seen, UnitFile};
use docket_core::helper::{HelperName, JobId};
use docket_core::routine::{JobEnd, RoutineRecord, RunRequest, Zone};
use prov::{HelperId, SpaceId, UnixSeconds};
use std::future::Future;

/// The scheduler's surroundings.
pub trait Host: Send + Sync + 'static {
    /// The local zone at `at`.
    fn zone(&self, at: UnixSeconds) -> Zone;

    /// The Space of a helper; none if it is gone.
    fn space_of(&self, helper: &HelperId) -> Option<SpaceId>;

    /// What the person calls a helper.
    fn name_of(&self, helper: &HelperId) -> Option<HelperName>;

    /// Posts the run as an ordinary job on its helper: its goal the person's words, its event a
    /// handle. The job's id, or none if the helper could not take it.
    fn post(&self, run: RunRequest) -> impl Future<Output = Option<JobId>> + Send;

    /// How a job ended, once it has.
    fn job_end(&self, job: JobId) -> Option<JobEnd>;

    /// The routines' stored records.
    fn read(&self, spaces: &[SpaceId]) -> impl Future<Output = Vec<RoutineRecord>> + Send;

    /// Stores records in a Space.
    fn save(&self, space: &SpaceId, records: Vec<RoutineRecord>)
    -> impl Future<Output = ()> + Send;

    /// Tells the person a sentence about a helper, once.
    fn say(&self, helper: &HelperId, text: String);

    /// The events the almanac holds for `ask`.
    fn events(&self, ask: EventAsk) -> impl Future<Output = Vec<Seen>> + Send;

    /// Makes the systemd user timers exactly `units` (the whole set, every time it changes).
    fn install(&self, units: &[UnitFile]);

    /// The desktop must stay awake, or may sleep. Said when it changes.
    fn awake(&self, need: Awake);
}
