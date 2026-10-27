//! Starting an agent installed from the registry: read the record `docket-agents` left in the
//! agents directory and make the agent's home. The values are `agents_core`'s; this is the only
//! disk access the launcher needs, so it does not link the installer.

use agents_core::dirs::AgentsDir;
use agents_core::launch::{Launch, LaunchFault};
use agents_core::record::{FILE, Record, RecordFault};
use agents_core::slug::Slug;

/// How to start `id` at `version`; makes its home when it is not there yet.
pub fn launch(dir: &AgentsDir, id: &str, version: &str) -> Result<Launch, LaunchFault> {
    let (slug, pinned) = (Slug::parse(id)?, Slug::parse(version)?);
    let at = dir.installed(&slug, &pinned);
    let record = std::fs::read_to_string(at.join(FILE))
        .map_err(|_| RecordFault::Missing)
        .and_then(|text| Record::parse(&text))
        .map_err(|why| LaunchFault::Record {
            id: id.to_owned(),
            version: version.to_owned(),
            why,
        })?;
    let home = dir.home(&slug);
    std::fs::create_dir_all(&home).map_err(|_| LaunchFault::Home)?;
    Ok(Launch::of(&record, &at, home))
}
