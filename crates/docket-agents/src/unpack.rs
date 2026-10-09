//! Unpacks an archive into a directory, refusing anything that would land outside it.

use flate2::read::GzDecoder;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

/// Why an archive was not unpacked.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnpackFault {
    /// Not a kind docket unpacks (`.tar.gz`, `.tgz` and `.zip` are).
    #[error("this kind of archive cannot be unpacked here")]
    Kind,
    /// An entry would land outside the directory, or is a link.
    #[error("the archive holds a path or link that is not allowed")]
    Path,
    /// The archive is damaged or the disk refused.
    #[error("the archive could not be unpacked")]
    Damaged,
}

/// Which archive a name says it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    TarGz,
    Zip,
}

fn kind(name: &str) -> Result<Kind, UnpackFault> {
    let name = name.split(['?', '#']).next().unwrap_or(name);
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Ok(Kind::TarGz)
    } else if name.ends_with(".zip") {
        Ok(Kind::Zip)
    } else {
        Err(UnpackFault::Kind)
    }
}

/// A relative path with no `..`, root or prefix.
fn inside(path: &Path) -> Option<PathBuf> {
    path.components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
        .then(|| path.to_owned())
}

/// Unpacks `bytes` (named `name`, which says the kind) into `into`.
pub fn unpack(name: &str, bytes: &[u8], into: &Path) -> Result<(), UnpackFault> {
    match kind(name)? {
        Kind::TarGz => tar_gz(bytes, into),
        Kind::Zip => zip(bytes, into),
    }
}

fn tar_gz(bytes: &[u8], into: &Path) -> Result<(), UnpackFault> {
    let mut archive = tar::Archive::new(GzDecoder::new(bytes));
    let entries = archive.entries().map_err(|_| UnpackFault::Damaged)?;
    for entry in entries {
        let mut entry = entry.map_err(|_| UnpackFault::Damaged)?;
        let kind = entry.header().entry_type();
        if !(kind.is_file() || kind.is_dir()) {
            return Err(UnpackFault::Path);
        }
        let path = entry.path().map_err(|_| UnpackFault::Damaged)?;
        inside(&path).ok_or(UnpackFault::Path)?;
        entry
            .unpack_in(into)
            .map_err(|_| UnpackFault::Damaged)
            .and_then(|done| done.then_some(()).ok_or(UnpackFault::Path))?;
    }
    Ok(())
}

fn zip(bytes: &[u8], into: &Path) -> Result<(), UnpackFault> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| UnpackFault::Damaged)?;
    for n in 0..archive.len() {
        let mut file = archive.by_index(n).map_err(|_| UnpackFault::Damaged)?;
        let link = file.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000);
        let rel = file
            .enclosed_name()
            .and_then(|p| inside(&p))
            .filter(|_| !link)
            .ok_or(UnpackFault::Path)?;
        let target = into.join(rel);
        if file.is_dir() {
            std::fs::create_dir_all(&target).map_err(|_| UnpackFault::Damaged)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|_| UnpackFault::Damaged)?;
        }
        let mut body = Vec::new();
        file.read_to_end(&mut body)
            .map_err(|_| UnpackFault::Damaged)?;
        std::fs::write(&target, body).map_err(|_| UnpackFault::Damaged)?;
        set_mode(&target, file.unix_mode())?;
    }
    Ok(())
}

fn set_mode(path: &Path, mode: Option<u32>) -> Result<(), UnpackFault> {
    use std::os::unix::fs::PermissionsExt;
    let Some(mode) = mode else { return Ok(()) };
    let perms = std::fs::Permissions::from_mode(mode & 0o777);
    std::fs::set_permissions(path, perms).map_err(|_| UnpackFault::Damaged)
}
