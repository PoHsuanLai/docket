//! Finished tasks whose router session stays open. A handle in an answer is shown through
//! `Session.Display` on the task's session, so the session outlives the task until the answer is
//! dismissed (`Companion1.Close`). The count is bounded, never the clock: when one more task
//! finishes than the bound allows, the oldest finished one is closed as a dismissal would.

use prov::TaskId;

/// How many finished tasks keep their router session open at once.
pub(crate) const LINGER: usize = 8;

/// The finished-but-open tasks, oldest finish first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Lingering(Vec<TaskId>);

/// What a finish asks for: the new queue and the tasks to close now, in finish order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Evicted {
    pub queue: Lingering,
    pub close: Vec<TaskId>,
}

impl Lingering {
    /// The queue holding `tasks`, oldest first.
    #[cfg(test)]
    pub(crate) fn of(tasks: &[&str]) -> Self {
        Self(
            tasks
                .iter()
                .map(|t| TaskId::parse(t).expect("task"))
                .collect(),
        )
    }
}

/// `task` finished: it joins the queue, and the oldest ones beyond `cap` are to be closed. A task
/// already queued keeps its place.
pub(crate) fn finished(queue: Lingering, task: TaskId, cap: usize) -> Evicted {
    let mut kept = queue.0;
    if !kept.contains(&task) {
        kept.push(task);
    }
    let over = kept.len().saturating_sub(cap);
    let close: Vec<TaskId> = kept.drain(..over).collect();
    Evicted {
        queue: Lingering(kept),
        close,
    }
}

/// `task` was dismissed (or closed for another reason): it no longer waits.
pub(crate) fn dismissed(queue: Lingering, task: &TaskId) -> Lingering {
    Lingering(queue.0.into_iter().filter(|t| t != task).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(t: &str) -> TaskId {
        TaskId::parse(t).expect("task")
    }

    type Row = (
        &'static [&'static str],
        &'static str,
        usize,
        &'static [&'static str],
        &'static [&'static str],
    );

    #[test]
    fn finish_table() {
        let table: [Row; 6] = [
            (&[], "t-1", 2, &["t-1"], &[]),
            (&["t-1"], "t-2", 2, &["t-1", "t-2"], &[]),
            (&["t-1", "t-2"], "t-3", 2, &["t-2", "t-3"], &["t-1"]),
            (&["t-1", "t-2"], "t-2", 2, &["t-1", "t-2"], &[]),
            (
                &["t-1", "t-2", "t-3"],
                "t-4",
                2,
                &["t-3", "t-4"],
                &["t-1", "t-2"],
            ),
            (&["t-1"], "t-2", 0, &[], &["t-1", "t-2"]),
        ];
        for (queue, task, cap, want_queue, want_close) in table {
            let got = finished(Lingering::of(queue), id(task), cap);
            assert_eq!(got.queue, Lingering::of(want_queue), "{queue:?} + {task}");
            let close: Vec<TaskId> = want_close.iter().map(|t| id(t)).collect();
            assert_eq!(got.close, close, "{queue:?} + {task}");
        }
    }

    #[test]
    fn dismissal_table() {
        let table: [(&[&str], &str, &[&str]); 3] = [
            (&["t-1", "t-2"], "t-1", &["t-2"]),
            (&["t-1", "t-2"], "t-9", &["t-1", "t-2"]),
            (&[], "t-1", &[]),
        ];
        for (queue, task, want) in table {
            assert_eq!(
                dismissed(Lingering::of(queue), &id(task)),
                Lingering::of(want)
            );
        }
    }
}
