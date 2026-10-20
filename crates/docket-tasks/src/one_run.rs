//! One run at a time. An on-screen run (`cua.run.start`, the `Tier::Cua` way in) holds its window
//! until it reports back, and a second start only reaches the person as another sheet before the
//! run daemon refuses it as busy. So a planner that already holds a live run of its own is
//! refused here, before the call is made: no sheet, no call, and the history says busy, which the
//! planner is told with the same hint as the daemon's own refusal.
//!
//! The run is live by the structure the task already keeps: a start that was done is a run, and a
//! report from a run in the task's inbox (anything but progress) is its end. Another task's runs
//! are not this task's; the daemon's own refusal still covers those.

use crate::task::TaskRuntime;
use docket_core::{ActionRef, StepEnd};
use prov::{AgentRef, MessageKind, ReportStatus};

/// The app whose runs are on screen.
const CUA_APP: &str = "org.quire.Cua";
/// The action that starts one.
const RUN_START: &str = "cua.run.start";

/// Whether `action` starts a run in an app that has one run at a time.
fn starts_screen_run(action: &ActionRef) -> bool {
    action.app.as_str() == CUA_APP && action.name.as_str() == RUN_START
}

impl TaskRuntime {
    /// Whether `action` starts a run this task already has live: more starts done than runs that
    /// have reported back.
    pub fn holds_run(&self, action: &ActionRef) -> bool {
        let started = self
            .history
            .iter()
            .filter(|s| s.action == *action && matches!(s.end, StepEnd::Done { .. }))
            .count();
        let ended = self
            .inbox
            .iter()
            .filter(|m| matches!(m.from.agent, AgentRef::Cua { .. }))
            .filter(|m| {
                matches!(m.kind, MessageKind::Report { status } if status != ReportStatus::Progress)
            })
            .count();
        starts_screen_run(action) && started > ended
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docket_core::{CallId, InboundLine, StepShown};
    use prov::{ActionName, Address, Crossing, Effect, MessageId, SpaceId, TaskId, ThreadId};

    fn action(app: &str, name: &str) -> ActionRef {
        ActionRef {
            app: porter_core::AppName::parse(app).expect("app"),
            name: ActionName::parse(name).expect("name"),
        }
    }

    fn start() -> ActionRef {
        action(CUA_APP, RUN_START)
    }

    fn line(action: ActionRef, end: StepEnd) -> docket_core::StepLine {
        docket_core::StepLine {
            call: CallId(0),
            action,
            effect: Effect::Read,
            end,
            shown: StepShown::Full,
            with: Vec::new(),
        }
    }

    fn done() -> StepEnd {
        StepEnd::Done {
            said: None,
            value: None,
            undo: None,
        }
    }

    fn report(status: ReportStatus, from: AgentRef) -> InboundLine {
        InboundLine {
            id: MessageId::parse("m-1").expect("id"),
            thread: ThreadId::parse("m-1").expect("thread"),
            to: Address::new(AgentRef::Companion, SpaceId::desktop()),
            from: Address::new(from, SpaceId::desktop()),
            crossing: Crossing::Within,
            kind: MessageKind::Report { status },
            parts: Vec::new(),
        }
    }

    fn cua() -> AgentRef {
        AgentRef::Cua {
            run: prov::RunId::parse("r-1").expect("run"),
        }
    }

    fn task(history: Vec<docket_core::StepLine>, inbox: Vec<InboundLine>) -> TaskRuntime {
        let mut rt = TaskRuntime::new(
            prov::SessionId::parse("s-1").expect("session"),
            SpaceId::desktop(),
            AgentRef::Worker {
                task: TaskId::parse("t-1").expect("task"),
            },
            None,
            prov::UnixSeconds(0),
        );
        rt.history = history;
        rt.inbox = inbox;
        rt
    }

    #[test]
    fn a_planner_holds_a_run_from_its_start_until_the_run_reports_back() {
        let started = || line(start(), done());
        let table = [
            ("nothing started", task(vec![], vec![]), false),
            ("a run live", task(vec![started()], vec![]), true),
            (
                "a start that did not run",
                task(vec![line(start(), StepEnd::Interrupted)], vec![]),
                false,
            ),
            (
                "progress is not the end",
                task(vec![started()], vec![report(ReportStatus::Progress, cua())]),
                true,
            ),
            (
                "the run reported done",
                task(vec![started()], vec![report(ReportStatus::Done, cua())]),
                false,
            ),
            (
                "the run reported failed, a second is live",
                task(
                    vec![started(), started()],
                    vec![report(ReportStatus::Failed, cua())],
                ),
                true,
            ),
            (
                "a report from another sender",
                task(
                    vec![started()],
                    vec![report(ReportStatus::Done, AgentRef::Companion)],
                ),
                true,
            ),
            (
                "another action of the app",
                task(vec![line(action(CUA_APP, "cua.run.stop"), done())], vec![]),
                false,
            ),
        ];
        for (name, rt, want) in table {
            assert_eq!(rt.holds_run(&start()), want, "case {name}");
        }
        // The check is for the run-start action only, however live the task's run is.
        let live = task(vec![line(start(), done())], vec![]);
        assert!(!live.holds_run(&action("org.quire.Mail", "mail.thread.archive")));
        // Another task has no run of its own: this task's run does not block it.
        assert!(!task(vec![], vec![]).holds_run(&start()));
    }
}
