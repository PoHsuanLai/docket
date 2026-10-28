//! The routines' host in the daemon: the hub's board and records, the almanac through the router,
//! the system's zone, and the systemd timers when a directory for them is given.

use super::host::Host;
use super::unit_dir::UnitDir;
use super::zone::SystemZone;
use crate::Hub;
use almanac_core::{BodyMode, RecentQuery, TrustFilter};
use companion_core::{Awake, EventAsk, Seen, UnitFile};
use companion_model::helper::{
    BoardEffect, BoardEvent, GoalOrigin, HelperName, HelperRecord, JobId, NewJob,
};
use companion_model::routine::{JobEnd, RoutineRecord, RunRequest, Zone};
use docket_client::Transport as IntentsTransport;
use docket_planner::Planner;
use docket_tasks::Surface;
use docket_vocab::Clock;
use porter_core::Count;
use prov::{HelperId, SpaceId, UnixSeconds};

/// The most events one question to the almanac returns.
const MOST_EVENTS: u32 = 200;

/// Where the systemd timers go, if anywhere.
#[derive(Debug, Clone)]
pub enum Timers {
    /// Not written: companiond's own timer runs the routines while it is up.
    Off,
    /// Written into this directory.
    Dir(UnitDir),
}

/// The hub as the routines' host.
pub struct HubHost<P: Planner, I: IntentsTransport, K: Clock, S: Surface> {
    hub: Hub<P, I, K, S>,
    zone: SystemZone,
    timers: Timers,
}

impl<P: Planner, I: IntentsTransport, K: Clock, S: Surface> std::fmt::Debug
    for HubHost<P, I, K, S>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubHost").finish_non_exhaustive()
    }
}

impl<P: Planner, I: IntentsTransport, K: Clock, S: Surface> HubHost<P, I, K, S> {
    /// The host over `hub`.
    pub fn new(hub: Hub<P, I, K, S>, timers: Timers) -> Self {
        Self {
            hub,
            zone: SystemZone,
            timers,
        }
    }
}

impl<P, I, K, S> Host for HubHost<P, I, K, S>
where
    P: Planner + 'static,
    I: IntentsTransport + 'static,
    K: Clock + 'static,
    S: Surface + Send + Sync + 'static,
{
    fn zone(&self, at: UnixSeconds) -> Zone {
        self.zone.at(at)
    }

    fn space_of(&self, helper: &HelperId) -> Option<SpaceId> {
        self.hub.helper_space(helper)
    }

    fn name_of(&self, helper: &HelperId) -> Option<HelperName> {
        self.hub.helper_name(helper)
    }

    async fn post(&self, run: RunRequest) -> Option<JobId> {
        let job = Box::new(NewJob {
            to: run.helper,
            goal: run.goal,
            // The goal is the person's words, written when the routine was made.
            origin: GoalOrigin::Person,
            giver: None,
            owns: Vec::new(),
            with: Vec::new(),
        });
        let event = BoardEvent::PostRun {
            job,
            about: run.event,
            steps: run.budget.steps,
        };
        self.hub
            .apply(event)
            .await
            .into_iter()
            .find_map(|effect| match effect {
                BoardEffect::Posted(id) => Some(id),
                _ => None,
            })
    }

    fn job_end(&self, job: JobId) -> Option<JobEnd> {
        self.hub.job_end(job)
    }

    async fn read(&self, spaces: &[SpaceId]) -> Vec<RoutineRecord> {
        self.hub
            .read_routine_records(spaces)
            .await
            .into_iter()
            .filter_map(|record| match record {
                HelperRecord::Routine(routine) => Some(*routine),
                _ => None,
            })
            .collect()
    }

    async fn save(&self, space: &SpaceId, records: Vec<RoutineRecord>) {
        let records: Vec<HelperRecord> = records
            .into_iter()
            .map(|record| HelperRecord::Routine(Box::new(record)))
            .collect();
        self.hub.write_records(space, &records).await;
    }

    fn say(&self, helper: &HelperId, text: String) {
        self.hub.say(helper, text);
    }

    async fn events(&self, ask: EventAsk) -> Vec<Seen> {
        let query = RecentQuery {
            since: ask.since,
            kinds: ask.kinds,
            trust: TrustFilter::Any,
            limit: Count(MOST_EVENTS),
            // Only the handle and the kind are read; an event's content is never fetched.
            bodies: BodyMode::Without,
        };
        self.hub
            .recent_in(&ask.space, query)
            .await
            .into_iter()
            .map(|entry| Seen {
                event: entry.summary.event,
                kind: entry.summary.kind.as_str().to_owned(),
            })
            .collect()
    }

    fn install(&self, units: &[UnitFile]) {
        let Timers::Dir(dir) = &self.timers else {
            return;
        };
        match dir.install(units) {
            Ok(changed) if !changed.is_empty() => {
                tracing::info!(
                    ?changed,
                    "routine timers written; systemd reads them on reload"
                );
            }
            Ok(_) => {}
            Err(cause) => tracing::warn!(%cause, "the routine timers could not be written"),
        }
    }

    fn awake(&self, need: Awake) {
        // No inhibit seam exists in the program yet: this is the typed hook. A host that gets one
        // takes the lock while `Needed` and drops it on `NotNeeded`.
        tracing::debug!(?need, "the desktop's need to stay awake changed");
    }
}
