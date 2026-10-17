//! The rules of the board that do not need its state: which goals may be instructions.

use super::card::HelperName;
use super::job::{GoalOrigin, GoalSource, PlannerSaw};
use serde::{Deserialize, Serialize};

/// Why a job was not made. Each has a plain sentence; none names how the board works.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum JobRefusal {
    /// There is no such helper.
    NoSuchHelper,
    /// The goal would come from something the giving planner had read, which is data and never
    /// an instruction to another.
    UntrustedGoal,
    /// Another live job already owns something this one would write.
    Owned(HelperName),
}

impl JobRefusal {
    /// The sentence the person (or the giving planner) is told.
    pub fn plain(&self) -> String {
        match self {
            JobRefusal::NoSuchHelper => "There is no helper by that name.".to_owned(),
            JobRefusal::UntrustedGoal => {
                "That came from something the assistant read, so it can't be handed to a helper as an instruction. Tell the helper yourself what to do.".to_owned()
            }
            JobRefusal::Owned(name) => super::notice::owned_sentence(name),
        }
    }
}

/// The two channels, never mixed: a goal is an instruction only when the person wrote it or when
/// the planner that wrote it had seen nothing untrusted. Everything else a helper derived from
/// untrusted content travels as a handle and can never become another helper's instruction.
pub fn admit_goal(origin: GoalOrigin) -> Result<GoalSource, JobRefusal> {
    match origin {
        GoalOrigin::Person => Ok(GoalSource::Person),
        GoalOrigin::Planner(PlannerSaw::Nothing) => Ok(GoalSource::PlannerOnTrusted),
        GoalOrigin::Planner(PlannerSaw::Untrusted) => Err(JobRefusal::UntrustedGoal),
    }
}
