//! A hang guard for a test executable. It is not a timing assertion: no test's outcome depends
//! on it. It exists so a test that waits forever (a lost wake-up, a peer that never answers)
//! ends the whole process loudly, instead of idling for days after its worktree is gone.

use std::sync::Once;
use std::time::Duration;

/// How long an executable may run before the guard ends it: far above any honest run.
const LIMIT: Duration = Duration::from_secs(20 * 60);

/// The exit code of a hung executable (that of `timeout`).
const HUNG: i32 = 124;

static ARMED: Once = Once::new();

/// Starts the guard once per process; later calls do nothing. Call it from the fixture every
/// test builds. Exiting skips `Drop`, so a bus the process held is ended by its watchdog.
pub fn arm() {
    ARMED.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("hang-guard".into())
            .spawn(|| {
                std::thread::sleep(LIMIT);
                eprintln!("hang guard: the test executable ran past {LIMIT:?}; ending it");
                std::process::exit(HUNG);
            });
        drop(spawned);
    });
}
