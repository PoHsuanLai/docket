//! The policy files, live: a `notify` watch on the person's, assistant's, administrator's and
//! preset directories (the directories, not the files: editors replace the inode), the same
//! debounce as the settings watch, then the whole set is built again and swapped into the router.
//! The next call decides by it.
//!
//! A rebuild that fails (an administrator file that does not load) leaves the set in force as it
//! is and says so; it is never replaced by a looser one. The administrator's directory is watched
//! only if it exists when the daemon starts.

use crate::policies::{PolicyDirs, load_policies};
use crate::settings_watch::DEBOUNCE;
use docket_router::{Router, Seams};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Builds the set again and puts it in force on `router`, logging what was left out.
pub fn reload<S: Seams>(router: &Router<S>, dirs: &PolicyDirs) {
    match load_policies(dirs) {
        Ok(layered) => {
            for rejected in &layered.skipped {
                eprintln!("intentd: skipped policy {rejected}");
            }
            router.set_pdp(layered.pdp);
        }
        Err(why) => eprintln!("intentd: policies not reloaded, the set in force stays: {why}"),
    }
}

/// The directories to watch: the person's two are made if missing, the others only watched if
/// they are there.
fn watched(dirs: &PolicyDirs) -> Vec<PathBuf> {
    let mine = [dirs.person.clone(), dirs.assistant()];
    for dir in &mine {
        // A missing directory that cannot be made is reported by the watch that follows.
        let _ = std::fs::create_dir_all(dir);
    }
    mine.into_iter()
        .chain(dirs.preset())
        .chain([dirs.admin.clone()])
        .filter(|d| d.is_dir())
        .collect()
}

/// Starts one watcher on `dir` that signals on every write. The watcher stops when dropped.
fn watch_dir(
    dir: &Path,
    signal: mpsc::UnboundedSender<()>,
) -> Result<RecommendedWatcher, notify::Error> {
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok_and(|event| !event.kind.is_access()) {
            // The receiver is gone once the daemon stops; nobody is left to tell.
            let _ = signal.send(());
        }
    })?;
    watcher.watch(dir, RecursiveMode::NonRecursive)?;
    Ok(watcher)
}

/// Rebuilds the set after every settled burst of changes, until the daemon stops.
pub async fn follow<S: Seams>(router: Arc<Router<S>>, dirs: PolicyDirs) {
    let (signal, mut events) = mpsc::unbounded_channel();
    let watchers: Vec<RecommendedWatcher> = watched(&dirs)
        .iter()
        .filter_map(|dir| match watch_dir(dir, signal.clone()) {
            Ok(watcher) => Some(watcher),
            Err(why) => {
                eprintln!(
                    "intentd: policies in {} are read once, not watched: {why}",
                    dir.display()
                );
                None
            }
        })
        .collect();
    drop(signal);
    if watchers.is_empty() {
        return;
    }
    while events.recv().await.is_some() {
        // Every further event restarts the window: a rename can fire more than one.
        loop {
            tokio::select! {
                () = tokio::time::sleep(DEBOUNCE) => break,
                more = events.recv() => if more.is_none() { return },
            }
        }
        reload(router.as_ref(), &dirs);
    }
    // `watchers` lives to here: dropping it ends the watches.
    drop(watchers);
}
