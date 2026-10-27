//! The agents directory on disk: the install record and the offered record read and written, and
//! what is installed listed. The paths are `agents_core::dirs::AgentsDir`'s; the values and their
//! text are `agents_core`'s.

use agents_core::dirs::AgentsDir;
use agents_core::offered::{Offered, OfferedFault};
use agents_core::record::{FILE, Record, RecordFault};
use agents_core::slug::Slug;
use docket_store::{read_optional, write_atomic};
use std::path::Path;

/// Writes the record into `dir`, the version's directory.
pub fn write_record(record: &Record, dir: &Path) -> std::io::Result<()> {
    let text = record.render().map_err(std::io::Error::other)?;
    std::fs::write(dir.join(FILE), text)
}

/// Reads the record in `dir`, the version's directory.
pub fn read_record(dir: &Path) -> Result<Record, RecordFault> {
    let text = std::fs::read_to_string(dir.join(FILE)).map_err(|_| RecordFault::Missing)?;
    Record::parse(&text)
}

/// The agents with at least one version installed, by id. Reads only this directory.
pub fn agents(dir: &AgentsDir) -> Vec<Slug> {
    let Ok(read) = std::fs::read_dir(dir.all_installed()) else {
        return Vec::new();
    };
    let mut found: Vec<Slug> = read
        .filter_map(|e| {
            let name = e.ok()?.file_name();
            Slug::parse(name.to_str()?).ok()
        })
        .filter(|id| !versions(dir, id).is_empty())
        .collect();
    found.sort();
    found
}

/// The versions of `id` that are installed.
pub fn versions(dir: &AgentsDir, id: &Slug) -> Vec<Slug> {
    let of = dir.installed_of(id);
    let Ok(read) = std::fs::read_dir(&of) else {
        return Vec::new();
    };
    let mut found: Vec<Slug> = read
        .filter_map(|e| {
            let slug = e
                .ok()?
                .file_name()
                .to_str()
                .and_then(|n| Slug::parse(n).ok())?;
            // Only a version with its record is installed; a staging directory has none.
            read_record(&of.join(slug.as_str())).ok().map(|_| slug)
        })
        .collect();
    found.sort();
    found
}

/// Reads the record of `id`; none when the agent never started.
pub fn read_offered(dir: &AgentsDir, id: &str) -> Result<Option<Offered>, OfferedFault> {
    let path = dir.offered(&Slug::parse(id).map_err(|_| OfferedFault::Name)?);
    let text = read_optional(&path).map_err(|_| OfferedFault::Io)?;
    text.map(|t| Offered::parse(&t)).transpose()
}

/// Replaces the record of `id` with `offered` (see [`Offered::after`] for its counter and the
/// models it keeps) and returns what was written.
pub fn write_offered(dir: &AgentsDir, id: &str, offered: Offered) -> Result<Offered, OfferedFault> {
    let path = dir.offered(&Slug::parse(id).map_err(|_| OfferedFault::Name)?);
    let before = read_offered(dir, id).ok().flatten();
    let next = offered.after(before.as_ref());
    write_atomic(&path, next.render()?.as_bytes()).map_err(|_| OfferedFault::Io)?;
    Ok(next)
}
