//! Installing a pinned version, and asking what a newer one would be. The network is reached
//! here and nowhere else, through the `Fetch` seam. Nothing updates by itself: a newer version
//! is installed only when the person names it.

use crate::dirs::AgentsDir;
use crate::fetch::{Fetch, FetchFault};
use crate::hash::Sha256Hex;
use crate::platform::Platform;
use crate::record::{Check, FILES, Record};
use crate::run::{Run, RunFault};
use crate::slug::{Slug, SlugRefused};
use crate::snapshot::{Binary, Listing, Package, Snapshot};
use crate::unpack::{UnpackFault, unpack};
use std::path::Path;

/// Why an install did not happen. Nothing is left half-installed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InstallFault {
    /// The list has no such agent.
    #[error("the agent list has no {0}")]
    Unknown(String),
    /// The list offers another version.
    #[error("the agent list offers {offered}, not {wanted}")]
    Version {
        /// What was asked for.
        wanted: String,
        /// What the list has.
        offered: String,
    },
    /// No way to install it on this computer.
    #[error("{0} cannot be installed on this computer")]
    Unsupported(String),
    /// The archive is not what the list said it is.
    #[error("the download does not match the list: expected {expected}, got {got}")]
    Digest {
        /// What the list said.
        expected: String,
        /// What arrived.
        got: String,
    },
    /// A name unfit for a directory.
    #[error(transparent)]
    Name(#[from] SlugRefused),
    /// The download failed.
    #[error(transparent)]
    Fetch(#[from] FetchFault),
    /// The archive did not unpack.
    #[error(transparent)]
    Unpack(#[from] UnpackFault),
    /// The package manager failed.
    #[error(transparent)]
    Run(#[from] RunFault),
    /// The disk refused.
    #[error("the agents directory could not be written")]
    Disk,
    /// The program the list names is not in what was unpacked.
    #[error("the download does not contain the program the list names")]
    NoProgram,
}

/// What the person asked to install.
#[derive(Debug, Clone, Copy)]
pub struct Want<'a> {
    /// The registry id.
    pub id: &'a str,
    /// The exact version.
    pub version: &'a str,
}

/// What the list offers against what is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// The list has no such agent.
    Unknown,
    /// The offered version is installed.
    Current(String),
    /// The offered version is not installed; these are (possibly none).
    Offered {
        /// The version on offer.
        offered: String,
        /// The versions installed.
        installed: Vec<String>,
    },
}

/// Compares the list with the agents directory. Reads only.
pub fn standing(dir: &AgentsDir, snapshot: &Snapshot, id: &str) -> Standing {
    let (Some(listing), Ok(slug)) = (snapshot.find(id), Slug::parse(id)) else {
        return Standing::Unknown;
    };
    let installed: Vec<String> = dir
        .versions(&slug)
        .iter()
        .map(|v| v.as_str().to_owned())
        .collect();
    if installed.contains(&listing.version) {
        Standing::Current(listing.version.clone())
    } else {
        Standing::Offered {
            offered: listing.version.clone(),
            installed,
        }
    }
}

/// Installs `want` from `snapshot`. Already installed is not an error and reaches nothing.
pub fn install(
    dir: &AgentsDir,
    snapshot: &Snapshot,
    want: Want<'_>,
    (fetch, run): (&dyn Fetch, &dyn Run),
) -> Result<Record, InstallFault> {
    let listing = snapshot
        .find(want.id)
        .ok_or_else(|| InstallFault::Unknown(want.id.to_owned()))?;
    if listing.version != want.version {
        return Err(InstallFault::Version {
            wanted: want.version.to_owned(),
            offered: listing.version.clone(),
        });
    }
    let (id, version) = (Slug::parse(&listing.id)?, Slug::parse(&listing.version)?);
    let target = dir.installed(&id, &version);
    if let Ok(done) = Record::read(&target) {
        return Ok(done);
    }
    let partial = target.with_file_name(format!(".{}.partial", version.as_str()));
    let _ = std::fs::remove_dir_all(&partial);
    std::fs::create_dir_all(partial.join(FILES)).map_err(|_| InstallFault::Disk)?;
    let made = fill(listing, &partial, (fetch, run));
    let record = made.and_then(|record| {
        record.write(&partial).map_err(|_| InstallFault::Disk)?;
        let _ = std::fs::remove_dir_all(&target);
        std::fs::rename(&partial, &target).map_err(|_| InstallFault::Disk)?;
        Ok(record)
    });
    if record.is_err() {
        let _ = std::fs::remove_dir_all(&partial);
    }
    record
}

fn fill(
    listing: &Listing,
    partial: &Path,
    (fetch, run): (&dyn Fetch, &dyn Run),
) -> Result<Record, InstallFault> {
    let files = partial.join(FILES);
    let unsupported = || InstallFault::Unsupported(listing.name.clone());
    let dist = &listing.distribution;
    let record = |command: String, args, env, digest: Option<String>, check| Record {
        id: listing.id.clone(),
        version: listing.version.clone(),
        command,
        args,
        env,
        digest,
        check,
    };
    if let Some(bin) = dist.binary.get(&Platform::current()) {
        let (digest, check) = binary(bin, &files, fetch)?;
        let found = files.join(&bin.cmd);
        found
            .is_file()
            .then_some(())
            .ok_or(InstallFault::NoProgram)?;
        return Ok(record(
            bin.cmd.clone(),
            bin.args.clone(),
            bin.env.clone(),
            Some(digest),
            check,
        ));
    }
    let package = dist.npx.as_ref().ok_or_else(unsupported)?;
    let command = npm(package, &files, run)?;
    Ok(record(
        command,
        package.args.clone(),
        package.env.clone(),
        None,
        Check::PackageManager,
    ))
}

fn binary(bin: &Binary, files: &Path, fetch: &dyn Fetch) -> Result<(String, Check), InstallFault> {
    let bytes = fetch.get(&bin.archive)?;
    let got = Sha256Hex::of(&bytes);
    let check = match &bin.sha256 {
        Some(text) => {
            let expected = Sha256Hex::parse(text).map_err(|_| InstallFault::Digest {
                expected: text.clone(),
                got: got.as_str().to_owned(),
            })?;
            if expected != got {
                return Err(InstallFault::Digest {
                    expected: expected.as_str().to_owned(),
                    got: got.as_str().to_owned(),
                });
            }
            Check::Declared
        }
        None => Check::FirstUse,
    };
    unpack(&bin.archive, &bytes, files)?;
    Ok((got.as_str().to_owned(), check))
}

/// `@scope/name@1.2.3` is `@scope/name`.
fn package_name(package: &str) -> &str {
    match package.rfind('@') {
        Some(at) if at > 0 => &package[..at],
        _ => package,
    }
}

/// Installs a node package under `files` and returns the path of its program, relative.
fn npm(package: &Package, files: &Path, run: &dyn Run) -> Result<String, InstallFault> {
    let args: Vec<String> = [
        "install",
        "--prefix",
        ".",
        "--ignore-scripts",
        "--no-audit",
        "--no-fund",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain([package.package.clone()])
    .collect();
    run.run("npm", &args, files)?;
    let name = package_name(&package.package);
    let manifest =
        std::fs::read_to_string(files.join("node_modules").join(name).join("package.json"))
            .map_err(|_| InstallFault::NoProgram)?;
    let json: serde_json::Value =
        serde_json::from_str(&manifest).map_err(|_| InstallFault::NoProgram)?;
    let bin = match json.get("bin") {
        Some(serde_json::Value::String(_)) => name.rsplit('/').next().map(str::to_owned),
        Some(serde_json::Value::Object(map)) => map.keys().next().cloned(),
        _ => None,
    };
    let bin = bin
        .filter(|b| Slug::parse(b).is_ok())
        .ok_or(InstallFault::NoProgram)?;
    Ok(format!("node_modules/.bin/{bin}"))
}
