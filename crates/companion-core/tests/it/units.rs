//! The systemd timer text for the routines that run at a time.

use companion_core::units_of;
use docket_core::routine::{RoutineId, Schedule, TimerEntry};

const GOAL: &str = "Summarise my unread mail and list what needs an answer";

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
