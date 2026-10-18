//! Routines in companiond: a due time posts one job, a time that came while the computer slept is
//! skipped and recorded (or run once on waking, when asked), three runs that do not finish pause a
//! routine and tell the person once, an event is handed over as a handle, a job's end starts what
//! waits for it, and the systemd timers are generated as text. The host is a fake: no clock, no
//! bus, no unit on disk.

use almanac_core::{EventRef, ReplicaId, Seq};
use companion_wire::{EventForm, RoutineForm, RoutineReply, WhenForm};
use companiond::{
    Awake, Clock, EventAsk, Host, RoutineDesk, Routines, Seen, UnitDir, UnitFile, units_of,
};
use docket_core::helper::{HelperName, JobFault, JobId};
use docket_core::routine::{
    JobEnd, Missed, RoutineId, RoutinePause, RoutineRecord, RoutineState, RunOutcome, RunRequest,
    Schedule, TimerEntry, Zone,
};
use prov::{HelperId, SpaceId, UnixSeconds};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

const H: i64 = 3_600;
const DAY: i64 = 86_400;
const GOAL: &str = "Summarise my unread mail and list what needs an answer";

#[derive(Default)]
struct Log {
    posted: Vec<RunRequest>,
    ends: BTreeMap<JobId, JobEnd>,
    said: Vec<String>,
    stored: Vec<RoutineRecord>,
    installed: Vec<Vec<UnitFile>>,
    awake: Vec<Awake>,
    events: Vec<Seen>,
}

/// The routines' surroundings, scripted: it records everything it is asked.
struct FakeHost {
    log: Mutex<Log>,
    helper: HelperId,
}

impl FakeHost {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            log: Mutex::new(Log::default()),
            helper: helper(),
        })
    }

    fn log(&self) -> MutexGuard<'_, Log> {
        self.log.lock().expect("log")
    }
}

impl Host for FakeHost {
    fn zone(&self, _: UnixSeconds) -> Zone {
        Zone::UTC
    }

    fn space_of(&self, helper: &HelperId) -> Option<SpaceId> {
        (*helper == self.helper).then(|| SpaceId::parse("work").expect("space"))
    }

    fn name_of(&self, helper: &HelperId) -> Option<HelperName> {
        (*helper == self.helper).then(|| HelperName::parse("Inbox").expect("name"))
    }

    async fn post(&self, run: RunRequest) -> Option<JobId> {
        let mut log = self.log();
        log.posted.push(run);
        Some(JobId(log.posted.len() as u64))
    }

    fn job_end(&self, job: JobId) -> Option<JobEnd> {
        self.log().ends.get(&job).copied()
    }

    async fn read(&self, _: &[SpaceId]) -> Vec<RoutineRecord> {
        self.log().stored.clone()
    }

    async fn save(&self, _: &SpaceId, records: Vec<RoutineRecord>) {
        self.log().stored.extend(records);
    }

    fn say(&self, _: &HelperId, text: String) {
        self.log().said.push(text);
    }

    async fn events(&self, _: EventAsk) -> Vec<Seen> {
        std::mem::take(&mut self.log().events)
    }

    fn install(&self, units: &[UnitFile]) {
        self.log().installed.push(units.to_vec());
    }

    fn awake(&self, need: Awake) {
        self.log().awake.push(need);
    }
}

fn helper() -> HelperId {
    serde_json::from_str("\"h-1\"").expect("helper id")
}

fn start(at: i64) -> (Routines<FakeHost>, Arc<FakeHost>, Arc<AtomicI64>) {
    let (clock, hand) = Clock::manual(UnixSeconds(at));
    let host = FakeHost::new();
    (Routines::new(host.clone(), clock), host, hand)
}

fn form(when: WhenForm, missed: Missed) -> RoutineForm {
    RoutineForm {
        helper: helper(),
        goal: GOAL.to_owned(),
        when,
        missed,
        steps: 8,
    }
}

fn daily(missed: Missed) -> RoutineForm {
    form(WhenForm::At("daily 08:00".to_owned()), missed)
}

async fn made(routines: &Routines<FakeHost>, form: RoutineForm) -> RoutineId {
    match routines.create(form).await {
        RoutineReply::Made(id) => id,
        other => panic!("a routine, not {other:?}"),
    }
}

fn outcomes(routines: &Routines<FakeHost>, id: RoutineId) -> Vec<RunOutcome> {
    let book = routines.book();
    let routine = book.routine(id).expect("routine");
    routine.runs.iter().map(|run| run.outcome).collect()
}

fn set(hand: &AtomicI64, now: i64) {
    hand.store(now, Ordering::SeqCst);
}

fn event(n: u64) -> EventRef {
    EventRef {
        space: SpaceId::parse("work").expect("space"),
        replica: ReplicaId([3; 16]),
        seq: Seq(n),
    }
}

#[tokio::test]
async fn a_due_time_posts_one_job_and_the_desktop_stays_awake_only_while_it_runs() {
    let (routines, host, hand) = start(7 * H);
    let id = made(&routines, daily(Missed::Skip)).await;
    set(&hand, 8 * H + 2);
    routines.tick().await;
    routines.tick().await;

    let posted = host.log().posted.clone();
    assert_eq!(posted.len(), 1);
    assert_eq!(posted[0].goal.as_str(), GOAL);
    assert_eq!(posted[0].event, None);
    assert_eq!(outcomes(&routines, id), [RunOutcome::Started]);
    assert_eq!(host.log().awake, [Awake::Needed]);

    host.log().ends.insert(JobId(1), JobEnd::Done);
    routines.tick().await;
    assert_eq!(outcomes(&routines, id), [RunOutcome::Done]);
    assert_eq!(host.log().awake, [Awake::Needed, Awake::NotNeeded]);
}

#[tokio::test]
async fn a_time_that_came_while_asleep_is_skipped_and_recorded() {
    let (routines, host, hand) = start(7 * H);
    let id = made(&routines, daily(Missed::Skip)).await;
    // The computer slept through eight o'clock and woke at ten.
    set(&hand, 10 * H);
    routines.tick().await;
    assert!(host.log().posted.is_empty());
    assert_eq!(outcomes(&routines, id), [RunOutcome::Skipped]);
    assert!(host.log().awake.is_empty());
}

#[tokio::test]
async fn a_time_that_came_while_asleep_runs_once_when_the_person_said_so() {
    let (routines, host, hand) = start(7 * H);
    made(&routines, daily(Missed::RunOnceOnWake)).await;
    set(&hand, 10 * H);
    routines.tick().await;
    routines.tick().await;
    assert_eq!(host.log().posted.len(), 1);
}

#[tokio::test]
async fn a_time_that_passed_while_the_assistant_was_down_is_skipped_on_start() {
    let (routines, host, _hand) = start(7 * H);
    let id = made(&routines, daily(Missed::Skip)).await;
    let stored = host.log().stored.clone();

    // A new process, at ten, over the same records.
    let (again, host2, hand2) = start(7 * H);
    host2.log().stored = stored;
    set(&hand2, 10 * H);
    again
        .restore(&[SpaceId::parse("work").expect("space")])
        .await;
    again.tick().await;
    assert!(host2.log().posted.is_empty());
    assert_eq!(outcomes(&again, id), [RunOutcome::Skipped]);
}

#[tokio::test]
async fn three_runs_that_do_not_finish_pause_the_routine_and_tell_the_person_once() {
    let (routines, host, hand) = start(7 * H);
    let id = made(&routines, daily(Missed::Skip)).await;
    for day in 0..4 {
        set(&hand, day * DAY + 8 * H + 1);
        routines.tick().await;
        let job = JobId(day as u64 + 1);
        host.log()
            .ends
            .insert(job, JobEnd::Failed(JobFault::NoAnswer));
        routines.tick().await;
    }
    assert_eq!(host.log().posted.len(), 3);
    let said = host.log().said.clone();
    assert_eq!(said.len(), 1);
    assert!(said[0].starts_with("A routine of Inbox"), "{}", said[0]);
    let book = routines.book();
    assert_eq!(
        book.routine(id).expect("routine").state,
        RoutineState::Paused(RoutinePause::Failed3)
    );
    // Paused, it no longer wants a timer: the last install is empty.
    assert!(host.log().installed.last().expect("installed").is_empty());
}

#[tokio::test]
async fn an_event_posts_a_job_whose_goal_is_the_persons_words_and_whose_event_is_a_handle() {
    let (routines, host, hand) = start(0);
    let watch = WhenForm::On(EventForm {
        kind: "mail.*".to_owned(),
        space: None,
        words: "New mail in Inbox".to_owned(),
    });
    made(&routines, form(watch, Missed::Skip)).await;
    host.log().events.push(Seen {
        event: event(11),
        kind: "mail.received".to_owned(),
    });
    set(&hand, 60);
    routines.tick().await;

    let posted = host.log().posted.clone();
    assert_eq!(posted.len(), 1);
    assert_eq!(posted[0].goal.as_str(), GOAL);
    assert_eq!(posted[0].event.as_ref().map(|e| e.seq), Some(Seq(11)));

    // The same event, told again, is not acted on twice.
    host.log().events.push(Seen {
        event: event(11),
        kind: "mail.received".to_owned(),
    });
    set(&hand, 120);
    routines.tick().await;
    assert_eq!(host.log().posted.len(), 1);
}

#[tokio::test]
async fn a_routine_after_a_job_starts_when_that_job_is_done_and_not_when_it_fails() {
    let (routines, host, hand) = start(0);
    made(&routines, form(WhenForm::AfterJob(JobId(9)), Missed::Skip)).await;
    set(&hand, 10);
    host.log()
        .ends
        .insert(JobId(9), JobEnd::Failed(JobFault::Crashed));
    routines.tick().await;
    assert!(host.log().posted.is_empty());

    host.log().ends.insert(JobId(9), JobEnd::Done);
    set(&hand, 20);
    routines.tick().await;
    routines.tick().await;
    assert_eq!(host.log().posted.len(), 1);
}

#[tokio::test]
async fn the_timers_follow_the_routines_and_a_removed_routine_leaves_none() {
    let (routines, host, _hand) = start(0);
    let id = made(&routines, daily(Missed::Skip)).await;
    let names: Vec<String> = host
        .log()
        .installed
        .last()
        .expect("installed")
        .iter()
        .map(|unit| unit.name.clone())
        .collect();
    assert_eq!(names, ["quire-routine-1.timer", "quire-routine-1.service"]);
    routines.remove(id).await;
    assert!(host.log().installed.last().expect("installed").is_empty());
}

#[tokio::test]
async fn a_routine_with_a_bad_time_or_no_helper_is_refused_in_plain_words() {
    let (routines, _host, _hand) = start(0);
    let bad = form(WhenForm::At("0 8 * * *".to_owned()), Missed::Skip);
    match routines.create(bad).await {
        RoutineReply::Refused(why) => assert!(why.contains("daily 08:00"), "{why}"),
        other => panic!("refused, not {other:?}"),
    }
    let mut nobody = daily(Missed::Skip);
    nobody.helper = serde_json::from_str("\"h-2\"").expect("helper id");
    assert!(matches!(
        routines.create(nobody).await,
        RoutineReply::Refused(_)
    ));
    assert!(routines.book().routines().is_empty());
}

#[test]
fn the_timer_text_is_a_calendar_that_does_not_make_up_missed_times() {
    let entries = [
        TimerEntry {
            routine: RoutineId(3),
            schedule: Schedule::parse("weekdays 08:30").expect("schedule"),
        },
        TimerEntry {
            routine: RoutineId(4),
            schedule: Schedule::parse("every 2 hours").expect("schedule"),
        },
    ];
    let units = units_of(&entries);
    let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "quire-routine-3.timer",
            "quire-routine-3.service",
            "quire-routine-4.timer",
            "quire-routine-4.service"
        ]
    );
    let timer = &units[0].text;
    assert!(
        timer.contains("OnCalendar=Mon..Fri *-*-* 08:30:00\n"),
        "{timer}"
    );
    assert!(timer.contains("Persistent=false\n"), "{timer}");
    assert!(timer.contains("Unit=quire-routine-3.service\n"), "{timer}");
    assert!(timer.contains("WantedBy=timers.target\n"), "{timer}");
    assert!(units[2].text.contains("OnCalendar=*-*-* 0/2:00:00\n"));
    let service = &units[1].text;
    assert!(service.contains("Type=oneshot\n"), "{service}");
    assert!(
        service.contains("ExecStart=/usr/bin/systemctl --user start companiond.service\n"),
        "{service}"
    );
    // No cron string, and none of the person's words, in a unit.
    assert!(!timer.contains("* * *") && !service.contains(GOAL));
}

#[test]
fn the_unit_directory_is_made_to_hold_exactly_the_wanted_units() {
    let scratch = tempfile::tempdir().expect("scratch dir");
    let dir = UnitDir::new(scratch.path().join("systemd/user"));
    let both = units_of(&[
        TimerEntry {
            routine: RoutineId(1),
            schedule: Schedule::parse("daily 08:00").expect("schedule"),
        },
        TimerEntry {
            routine: RoutineId(2),
            schedule: Schedule::parse("daily 09:00").expect("schedule"),
        },
    ]);
    assert_eq!(dir.install(&both).expect("install").len(), 4);
    // Nothing changed: nothing written.
    assert!(dir.install(&both).expect("install").is_empty());
    std::fs::write(dir.path().join("other.service"), "not ours").expect("other file");
    let one = units_of(&[TimerEntry {
        routine: RoutineId(1),
        schedule: Schedule::parse("daily 08:00").expect("schedule"),
    }]);
    let mut changed = dir.install(&one).expect("install");
    changed.sort();
    assert_eq!(
        changed,
        ["quire-routine-2.service", "quire-routine-2.timer"]
    );
    assert!(
        dir.path().join("other.service").exists(),
        "not ours, not removed"
    );
}
