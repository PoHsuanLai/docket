//! A helper's recent jobs as the updates `session/load` replays: the person's goal, then what
//! became of it, in plain words. Pure: jobs in, protocol values out.

use crate::calls;
use agent_client_protocol::schema::v1::SessionUpdate;
use companion_model::helper::HelperName;
use companion_wire::{JobLine, JobShown};

/// How many of the latest jobs a load replays.
const RECENT: usize = 5;

fn status(name: &str, state: JobShown) -> String {
    match state {
        JobShown::Waiting => format!("{name} has not started this yet."),
        JobShown::Working => format!("{name} is working on this."),
        JobShown::Done => format!("{name} finished this."),
        JobShown::NeedsModel => {
            format!("{name} needs a model before it can start. Choose one for it.")
        }
        JobShown::NeedsSignIn => {
            format!("{name} is waiting for you to sign in to its app before it can start.")
        }
    }
}

fn job(name: &HelperName, job: &JobLine) -> Vec<SessionUpdate> {
    let mut updates = vec![
        calls::said(job.goal.clone()),
        calls::say(status(name.as_str(), job.state)),
    ];
    updates.extend(job.note.iter().cloned().map(calls::say));
    updates.extend(job.answer.iter().cloned().map(calls::say));
    updates
}

/// The updates that replay the latest jobs of the helper called `name`, oldest first.
pub(crate) fn recent(name: &HelperName, jobs: &[JobLine]) -> Vec<SessionUpdate> {
    let from = jobs.len().saturating_sub(RECENT);
    jobs[from..].iter().flat_map(|j| job(name, j)).collect()
}
