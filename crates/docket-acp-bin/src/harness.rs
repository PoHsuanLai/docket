//! What the acceptance harness needs to run the agent host: its entry point with the seams only a
//! harness chooses, and the pieces `docket-live` builds a host from in its own process. Behind the
//! `testing` feature; no packaged binary turns it on. The host itself is private to this crate.

/// Hosting one agent session in the harness's process.
pub mod host {
    pub use crate::agent::host::{HostError, Hosted, Wiring, host};
    pub use crate::agent::places::Places;
    pub use crate::agent::wire::SandboxLimits;
}

/// `docket-agent --serve` as the harness's own process.
pub mod serve {
    pub use crate::agent::serve::{HarnessSeams, run_with};
}

/// `docket-acp` as the harness's own process.
pub mod acp {
    use crate::{Callers, LiveFault};
    use docket_acp::Permit;
    use porter_daemon::ProcRoot;
    use std::path::PathBuf;

    /// What only the acceptance harness's mains choose: the `/proc` callers are read from (a
    /// fixture tree has each unit's main process in `<root>/units`) and the system's callers file.
    /// The packaged binary uses the system's `/proc` and `/etc/quire`, and no environment variable
    /// can change either.
    #[derive(Debug, Clone)]
    pub struct HarnessSeams {
        /// Where callers' processes are read: the system's `/proc`, or a fixture tree.
        pub proc_root: ProcRoot,
        /// The system's callers file, else `/etc/quire/acp-callers.toml`.
        pub system_callers: Option<PathBuf>,
    }

    impl Default for HarnessSeams {
        fn default() -> Self {
            Self {
                proc_root: ProcRoot::Unset,
                system_callers: None,
            }
        }
    }

    /// [`crate::serve`] with the seams the acceptance harness's mains choose.
    pub async fn serve_with(
        permit: Permit,
        trace: docket_dbus::InferWatch,
        seams: HarnessSeams,
        env: &impl Fn(&str) -> Option<String>,
    ) -> Result<(), LiveFault> {
        let HarnessSeams {
            proc_root,
            system_callers,
        } = seams;
        crate::serve_on(
            permit,
            trace,
            Callers {
                proc_root,
                system_callers,
            },
            env,
        )
        .await
    }
}
