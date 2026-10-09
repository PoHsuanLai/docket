//! The kit over docket-tasks' fakes: the in-process router with the fake mail app, and the
//! scripted planner model that remembers every request it was asked.
#![allow(dead_code)]

#[path = "../../../docket-tasks/tests/it/support/mod.rs"]
mod tasks;

pub use tasks::infer::{Say, ScriptedInfer, call, words};
pub use tasks::{ARCHIVE, World, app, thread, work};

use docket_client::{InProcess, Intents};
use docket_core::{AgentConfig, CallerId, CallerRole, TurnSource};
use docket_fake::FakeSeams;
use docket_kit::{Actions, Agent, AgentBuilder, Asker};
use docket_planner::Catalogue;
use porter_core::{AppId, Isolation};
use prov::UnixSeconds;
use std::collections::BTreeSet;

pub type Link = Intents<InProcess<FakeSeams>>;

/// The link the agent reaches the router by: the caller plays the editor and the companion, as
/// the companion's own tests do.
pub fn link(world: &World) -> Link {
    let caller = CallerId {
        app: AppId {
            name: app("org.quire.Acp"),
            isolation: Isolation::Unsandboxed,
        },
        roles: BTreeSet::from([CallerRole::Editor, CallerRole::Companion]),
    };
    Intents::over(InProcess::new(world.router.clone(), caller))
}

/// The catalogue of the manifests the router holds.
pub async fn catalogue(link: &Link) -> Catalogue {
    let manifests = link.manifests().await.expect("manifests");
    Catalogue::from_manifests(&manifests)
}

/// A builder over the world, with every mail action chosen.
pub async fn mail_agent(world: &World) -> AgentBuilder<ScriptedInfer, InProcess<FakeSeams>> {
    let link = link(world);
    let actions = Actions::from(&catalogue(&link).await).app("org.quire.Mail");
    Agent::builder(world.infer.clone(), link).actions(actions)
}

pub fn asker() -> Asker {
    Asker {
        app: app("org.quire.Shell"),
        space: work(),
        from: TurnSource::Editor(app("org.quire.Acp")),
        at: UnixSeconds(1_000),
    }
}

pub fn budget() -> docket_core::AssemblerBudget {
    AgentConfig::default().assembler
}
