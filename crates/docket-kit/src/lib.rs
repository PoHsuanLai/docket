//! Declaring an internal agent in a few lines (a reviewer, a policy writer, a worker, a test
//! agent) without a second loop. The kit is a facade over the machines docket already has:
//! `agent-loop`'s `agent_step` and `assemble`, `docket-planner`'s prompt and catalogue,
//! `docket-tasks`' memory reads and reader step, and the `Intents1` link. It adds typed choices
//! and the small driver that joins them for one task.
//!
//! What the kit does not have is as much of its design as what it has: an agent holds no tool
//! and no closure, so its only effect is a `CallRequest` sent through the router, which gates it
//! (labels, Cedar, review, confirmation, breaker, audit); nothing rewrites a call; a value a
//! call returns is a handle, never text in the next prompt; the rules in the system message
//! come first and are not an option of the builder.
//!
//! - [`Agent`], [`AgentBuilder`], [`Asker`]: declare and ask.
//! - [`Actions`]: the chosen actions, by app, name and greatest effect.
//! - [`Memory`], [`RecallScope`]: the sections it remembers, each opt-in.
//! - [`Limits`]: how far an ask may go.
//! - [`Run`], [`Ended`]: what an ask came to.
//! - The types the builder names are re-exported ([`Catalogue`], [`Tier`], [`TrustFilter`],
//!   [`Intents`], ...), so a caller needs this crate and a model transport alone.
//!
//! ```
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use docket_core::{AgentConfig, CallerId, CallerRole, TurnSource};
//! use docket_fake::{WordsModel, fake_router};
//! use docket_client::InProcess;
//! use docket_kit::{Actions, Agent, Asker, Catalogue, Ended, Intents};
//! use porter_core::{AppId, AppName, Isolation};
//! use prov::{SpaceId, UnixSeconds};
//! use std::{collections::BTreeSet, sync::Arc};
//!
//! // The router, in this process, with the neutral fake apps (Files among them).
//! let router = Arc::new(fake_router(AgentConfig::default())?);
//! let me = AppName::parse("org.quire.Shell")?;
//! let caller = CallerId {
//!     app: AppId { name: me.clone(), isolation: Isolation::Unsandboxed },
//!     roles: BTreeSet::from([CallerRole::Editor, CallerRole::Companion]),
//! };
//! let link = Intents::over(InProcess::new(router, caller));
//! let catalogue = Catalogue::from_manifests(&link.manifests().await?);
//!
//! let agent = Agent::builder(WordsModel::says("Nothing to change."), link)
//!     .role("You look over the files and say what you see.")
//!     .actions(Actions::from(&catalogue).app("org.quire.Files").reads_only())
//!     .build()?;
//! let asker = Asker {
//!     app: me.clone(),
//!     space: SpaceId::parse("work")?,
//!     from: TurnSource::Editor(me),
//!     at: UnixSeconds(1_000),
//! };
//! let run = agent.ask(&asker, "Anything to tidy?").await?;
//! assert_eq!(run.ended, Ended::Done);
//! assert_eq!(run.said, ["Nothing to change."]);
//! # Ok(()) }
//! ```

mod actions;
mod agent;
mod builder;
mod driver;
mod fault;
mod limits;
mod memory;
mod run;

pub use actions::{Actions, Missing};
pub use agent::{Agent, Asker};
pub use almanac_core::TrustFilter;
pub use builder::AgentBuilder;
pub use docket_client::{ClientError, Intents, Transport as IntentsTransport};
pub use docket_core::AssemblerBudget;
pub use docket_planner::Catalogue;
pub use fault::{BuildFault, KitFault};
pub use limits::Limits;
pub use memory::{EPISODE_LIMIT, Memory, RECALL_HITS, RecallScope};
pub use porter_client::Transport as InferTransport;
pub use porter_core::{Count, Tier};
pub use run::{Ended, Run};
