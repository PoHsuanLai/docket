//! The fake `/proc` of the scratch root: which process is which unit or app scope, as the
//! daemons' caller tables read it.

use super::World;
use super::files::write;
use std::path::Path;

/// What a process of the fake proc root is: the unit it runs in, or the app scope it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cgroup<'a> {
    /// A user service unit, `<name>.service`.
    Unit(&'a str),
    /// An app's scope, `app-<app>-1.scope`.
    AppScope(&'a str),
    /// A named transient user scope outside the `app-` namespace, `<name>.scope`.
    Scope(&'a str),
}

impl Cgroup<'_> {
    /// The one line of `/proc/<pid>/cgroup`.
    pub(crate) fn line(self) -> String {
        let slice = "0::/user.slice/user-1000.slice/user@1000.service/app.slice";
        match self {
            Cgroup::Unit(name) => format!("{slice}/{name}.service\n"),
            Cgroup::AppScope(app) => format!("{slice}/app-{app}-1.scope\n"),
            Cgroup::Scope(name) => format!("{slice}/{name}.scope\n"),
        }
    }
}

/// Puts process `pid` in the fake proc root `<root>/proc`, in `cgroup`. In a service unit it is
/// that unit's main process (`<root>/proc/units/<name>.service`, the fixture's stand-in for the
/// systemd manager's `MainPID`, which is all a caller table's service row names).
pub fn place(root: &Path, pid: u32, cgroup: Cgroup<'_>) {
    write(&root.join(format!("proc/{pid}/cgroup")), &cgroup.line());
    match cgroup {
        Cgroup::Unit(name) => write(
            &root.join(format!("proc/units/{name}.service")),
            &pid.to_string(),
        ),
        Cgroup::Scope(name) => link_image(&root.join(format!("proc/{pid}")), name)
            .unwrap_or_else(|e| panic!("{}: {e}", root.display())),
        Cgroup::AppScope(_) => {}
    }
}

/// Links `<pid_dir>/exe` to `/usr/bin/<name>`: a named scope has no main process, so the program
/// it runs is what its caller row matches (`exe`). A unit's main process needs none: the fixture
/// gives it the image its row names.
pub fn link_image(pid_dir: &Path, name: &str) -> std::io::Result<()> {
    let link = pid_dir.join("exe");
    match std::fs::remove_file(&link) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    std::os::unix::fs::symlink(format!("/usr/bin/{name}"), link)
}

/// Runs `work` with the harness process standing in as the app `app` (it is in the app's scope,
/// a plain app of that name), then puts it back as the shell: every connection of the harness is
/// one process, so who it is for the daemons is what the fake proc root says when they ask.
pub async fn as_app<T>(world: &World, app: &str, work: impl std::future::Future<Output = T>) -> T {
    let (root, pid) = (world.dir.path(), std::process::id());
    place(root, pid, Cgroup::AppScope(app));
    let done = work.await;
    place(root, pid, Cgroup::Unit("sill"));
    done
}

/// The pid the fake proc root names as the main process of service unit `name`, if any.
pub fn unit_main(root: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(root.join(format!("proc/units/{name}.service"))).ok()
}

/// Puts back the main process of service unit `name` that [`unit_main`] read (nothing: removes
/// the row). A process placed in a unit by [`place`] takes the unit's main pid; this gives it
/// back to the daemon that really is the unit.
pub fn restore_unit(root: &Path, name: &str, main: Option<String>) {
    let row = root.join(format!("proc/units/{name}.service"));
    match main {
        Some(pid) => write(&row, &pid),
        None => drop(std::fs::remove_file(row)),
    }
}
