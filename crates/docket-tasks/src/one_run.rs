//! One run at a time. An on-screen run (`cua.run.start`, the `Tier::Cua` way in) holds its window
//! until it reports back, and a second start only reaches the person as another sheet before the
//! run daemon refuses it as busy. So a planner that already holds a live run of its own is
//! refused here, before the call is made: no sheet, no call, and the history says busy, which the
//! planner is told with the same hint as the daemon's own refusal.
//!
//! The runs are counted as they happen ([`LiveRuns`]): a start that was done adds one, a report
//! from a run that lands (anything but progress) takes one away. The count is not stored, so a
//! task brought back after a restart holds no run: a report that fell out of the restart window
//! can never leave it refused for good, and the daemon's own refusal still covers a run that
//! outlived the restart. Another task's runs are not this task's.

use crate::task::TaskRuntime;
use docket_core::{ActionRef, InboundLine, StepEnd};
use prov::{AgentRef, MessageKind, ReportStatus};

/// The app whose runs are on screen.
const CUA_APP: &str = "org.quire.Cua";
/// The action that starts one.
const RUN_START: &str = "cua.run.start";

/// How many runs a task started in this process that have not reported back.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LiveRuns(u32);

/// Whether `action` starts a run in an app that has one run at a time.
fn starts_screen_run(action: &ActionRef) -> bool {
    action.app.as_str() == CUA_APP && action.name.as_str() == RUN_START
}

/// Whether `line` is a run's last word: a report from a run that is not progress.
fn ends_run(line: &InboundLine) -> bool {
    matches!(line.from.agent, AgentRef::Cua { .. })
        && matches!(line.kind, MessageKind::Report { status } if status != ReportStatus::Progress)
}

impl TaskRuntime {
    /// Whether `action` starts a run while this task already has one live.
    pub fn holds_run(&self, action: &ActionRef) -> bool {
        starts_screen_run(action) && self.live_runs != LiveRuns::default()
    }

    /// Counts a step that just ended: a run start that was done is a live run.
    pub(crate) fn note_step(&mut self, action: &ActionRef, end: &StepEnd) {
        if starts_screen_run(action) && matches!(end, StepEnd::Done { .. }) {
            self.live_runs = LiveRuns(self.live_runs.0.saturating_add(1));
        }
    }

    /// Counts a message that just landed: a run's report ends one live run.
    pub(crate) fn note_message(&mut self, line: &InboundLine) {
        if ends_run(line) {
            self.live_runs = LiveRuns(self.live_runs.0.saturating_sub(1));
        }
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
        for step in &history {
            rt.note_step(&step.action, &step.end);
        }
        for message in &inbox {
            rt.note_message(message);
        }
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
        // A task brought back after a restart has its history but no count: a start whose report
        // fell out of the restart window does not hold it.
        let mut restored = task(vec![], vec![]);
        restored.history = vec![line(start(), done())];
        assert!(!restored.holds_run(&start()));
    }
}
