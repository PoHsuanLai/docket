//! A short name for a world's runtime directory. A Unix socket's path is at most 107 bytes, and a
//! scratch root kept under a lane or run directory is already most of that, so an engine's socket
//! (`<run>/inferd/vllm-<model id>.sock`) would not fit. `XDG_RUNTIME_DIR` is therefore a link in
//! `/tmp` named after the scratch root, pointing at `<scratch>/run`: the sockets still live in the
//! scratch root, only the name every daemon is given is short.

use std::path::{Path, PathBuf};

/// The link's path: `/tmp/dl-<16 hex digits of the scratch root's FNV-1a hash>`.
pub(crate) fn short_run(dir: &Path) -> PathBuf {
    let hash = dir
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
        });
    PathBuf::from(format!("/tmp/dl-{hash:016x}"))
}

/// Points the short name at `<dir>/run`, replacing a stale link of an earlier run of the same root.
pub(crate) fn link_run(dir: &Path) -> std::io::Result<()> {
    let link = short_run(dir);
    unlink_run(dir);
    std::os::unix::fs::symlink(dir.join("run"), link)
}

/// Removes the short name if it is a link; anything else at that path is left alone.
pub(crate) fn unlink_run(dir: &Path) {
    let link = short_run(dir);
    if link
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        let _ = std::fs::remove_file(link);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_short_name_is_short_stable_and_per_root() {
        let long = Path::new(
            "/home/someone/rs-wt/live-eval/tmp/docket-live.bkxE6l/out/scratch/world-5Vb5Za",
        );
        let name = short_run(long);
        assert_eq!(name, short_run(long));
        assert_ne!(name, short_run(Path::new("/elsewhere/world-5Vb5Za")));
        let socket = name.join("inferd/vllm-qwen3-4b-instruct-2507-fp8.sock");
        assert!(socket.as_os_str().len() < 108, "{}", socket.display());
    }

    #[test]
    fn the_link_reaches_the_scratch_run_dir_and_goes() {
        let root = tempfile::tempdir().expect("root");
        std::fs::create_dir(root.path().join("run")).expect("run");
        link_run(root.path()).expect("link");
        std::fs::write(short_run(root.path()).join("probe"), b"x").expect("through the link");
        assert!(root.path().join("run/probe").exists());
        link_run(root.path()).expect("a second link replaces the first");
        unlink_run(root.path());
        assert!(short_run(root.path()).symlink_metadata().is_err());
    }
}
