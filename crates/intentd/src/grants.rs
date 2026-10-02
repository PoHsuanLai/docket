//! The person's standing consent for actions, in a file under the user's config directory.

use docket_core::ActionGrant;
use docket_router::GrantStore;
use std::path::PathBuf;

/// Grants kept in `$XDG_DATA_HOME/quire/intents/grants.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileGrants {
    path: PathBuf,
}

impl FileGrants {
    /// Grants stored at `path`.
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }
}

impl GrantStore for FileGrants {
    fn grants(&self) -> Vec<ActionGrant> {
        let _ = &self.path;
        todo!(
            "FileGrants::grants: read the file; a missing file is no grants, a malformed one is no grants and a log line, never a panic"
        )
    }

    fn record(&self, grant: ActionGrant) {
        let _ = (&self.path, grant);
        todo!(
            "FileGrants::record: add the grant and write the file atomically (temporary file, fsync, rename)"
        )
    }
}
