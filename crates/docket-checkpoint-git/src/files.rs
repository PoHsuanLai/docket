//! The only writes to the folder: deleting the files a restore removes, with the checks that no
//! path leaves the folder through a link. Blocking; the store runs these on a blocking thread.

use docket_core::WorkPath;
use std::io::ErrorKind;
use std::path::Path;

/// A path that would reach outside the folder, or a folder that could not be looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Unsafe {
    /// A folder on the way to the file is a link.
    #[error("a folder on the way is a link")]
    Link,
    /// A folder on the way could not be read.
    #[error("a folder on the way could not be read")]
    Unreadable,
}

/// Fails if any folder between `root` and a file of `paths` is a link: git would refuse to write
/// through one, and a delete must not follow one out of the folder either.
pub(crate) fn stay_inside(root: &Path, paths: &[&WorkPath]) -> Result<(), Unsafe> {
    for path in paths {
        let steps: Vec<&str> = path.as_str().split('/').collect();
        let mut here = root.to_path_buf();
        for step in steps.iter().take(steps.len().saturating_sub(1)) {
            here.push(step);
            match std::fs::symlink_metadata(&here) {
                Ok(meta) if meta.file_type().is_symlink() => return Err(Unsafe::Link),
                Ok(_) => {}
                // Nothing here yet, so nothing below it either.
                Err(why) if why.kind() == ErrorKind::NotFound => break,
                Err(_) => return Err(Unsafe::Unreadable),
            }
        }
    }
    Ok(())
}

/// Deletes `paths` (a path already gone is fine) and then the folders that are left empty by it,
/// never `root` itself.
pub(crate) fn delete(root: &Path, paths: &[WorkPath]) -> std::io::Result<()> {
    for path in paths {
        let file = root.join(path.as_str());
        match std::fs::remove_file(&file) {
            Ok(()) => {}
            Err(why) if why.kind() == ErrorKind::NotFound => {}
            Err(why) => return Err(why),
        }
        let mut folder = file.parent();
        while let Some(dir) = folder {
            if dir == root || !dir.starts_with(root) || std::fs::remove_dir(dir).is_err() {
                break;
            }
            folder = dir.parent();
        }
    }
    Ok(())
}
