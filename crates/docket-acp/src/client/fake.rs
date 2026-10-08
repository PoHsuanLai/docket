//! For tests: an in-memory pipe, a scripted `Spawn` and a file system in a map. No process, no
//! clock, no network.

use super::files::{FileFault, Files};
use super::spawn::{AgentChild, LaunchPlan, SessionMeta, Spawn, SpawnFault, Spawned};
use crate::wire::{Wire, WireClosed};
use docket_core::{AbsPath, Cover};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// One end of an in-memory pipe of lines.
#[derive(Debug)]
pub struct ChannelWire {
    rx: UnboundedReceiver<String>,
    tx: UnboundedSender<String>,
}

/// Two ends: what one writes the other reads.
pub fn pipe() -> (ChannelWire, ChannelWire) {
    let (a_tx, b_rx) = unbounded_channel();
    let (b_tx, a_rx) = unbounded_channel();
    (
        ChannelWire { rx: a_rx, tx: a_tx },
        ChannelWire { rx: b_rx, tx: b_tx },
    )
}

impl Wire for ChannelWire {
    async fn read_line(&mut self) -> Option<String> {
        self.rx.recv().await
    }

    async fn write_line(&mut self, line: String) -> Result<(), WireClosed> {
        self.tx.send(line).map_err(|_| WireClosed)
    }
}

/// What the fake spawner saw.
#[derive(Debug, Default)]
pub struct SpawnLog {
    /// Every plan it was asked to start.
    pub plans: Vec<LaunchPlan>,
    /// How many children were killed.
    pub killed: usize,
    /// How many children were closed (given back what they were lent).
    pub closed: usize,
}

/// A shared view of the spawner's log.
#[derive(Debug, Clone, Default)]
pub struct SpawnSeen(Arc<Mutex<SpawnLog>>);

impl SpawnSeen {
    /// The plans it was asked to start.
    pub fn plans(&self) -> Vec<LaunchPlan> {
        locked(&self.0).plans.clone()
    }

    /// Children killed.
    pub fn killed(&self) -> usize {
        locked(&self.0).killed
    }

    /// Children closed.
    pub fn closed(&self) -> usize {
        locked(&self.0).closed
    }
}

/// A scripted `Spawn`: hands out the wires it was given, one per start.
#[derive(Debug)]
pub struct FakeSpawn {
    wires: VecDeque<ChannelWire>,
    seen: SpawnSeen,
    meta: Option<SessionMeta>,
}

impl FakeSpawn {
    /// A spawner whose starts return `wires` in order, and a view of what it saw.
    pub fn new(wires: Vec<ChannelWire>) -> (Self, SpawnSeen) {
        let seen = SpawnSeen::default();
        (
            Self {
                wires: wires.into(),
                seen: seen.clone(),
                meta: None,
            },
            seen,
        )
    }

    /// The same spawner, handing back `meta` for `session/new` with every start.
    pub fn with_meta(mut self, meta: SessionMeta) -> Self {
        self.meta = Some(meta);
        self
    }
}

/// The fake's child.
#[derive(Debug)]
pub struct FakeChild {
    seen: SpawnSeen,
    killed: bool,
    closed: bool,
}

impl AgentChild for FakeChild {
    fn kill(&mut self) {
        if !self.killed {
            self.killed = true;
            locked(&self.seen.0).killed += 1;
        }
    }

    async fn close(&mut self) {
        self.kill();
        if !self.closed {
            self.closed = true;
            locked(&self.seen.0).closed += 1;
        }
    }
}

impl Spawn for FakeSpawn {
    type Wire = ChannelWire;
    type Child = FakeChild;

    async fn spawn(
        &mut self,
        plan: &LaunchPlan,
    ) -> Result<Spawned<ChannelWire, FakeChild>, SpawnFault> {
        locked(&self.seen.0).plans.push(plan.clone());
        let wire = self.wires.pop_front().ok_or(SpawnFault::Process)?;
        let child = FakeChild {
            seen: self.seen.clone(),
            killed: false,
            closed: false,
        };
        Ok(Spawned {
            wire,
            child,
            meta: self.meta.clone(),
        })
    }
}

#[derive(Debug, Default)]
struct Disk {
    files: BTreeMap<AbsPath, String>,
    links: Vec<(AbsPath, AbsPath)>,
    reads: usize,
    writes: Vec<(AbsPath, String)>,
}

/// A file system in a map, shared between the test and the backend.
#[derive(Debug, Clone, Default)]
pub struct FakeFiles(Arc<Mutex<Disk>>);

impl FakeFiles {
    /// An empty disk.
    pub fn new() -> Self {
        Self::default()
    }

    /// Puts a file there.
    pub fn put(&self, path: &AbsPath, text: &str) {
        locked(&self.0).files.insert(path.clone(), text.to_owned());
    }

    /// Makes everything at or below `at` really live below `to` (a symbolic link).
    pub fn link(&self, at: &AbsPath, to: &AbsPath) {
        locked(&self.0).links.push((at.clone(), to.clone()));
    }

    /// The text of a file, if there is one.
    pub fn text(&self, path: &AbsPath) -> Option<String> {
        locked(&self.0).files.get(path).cloned()
    }

    /// How many reads reached the disk.
    pub fn reads(&self) -> usize {
        locked(&self.0).reads
    }

    /// Every write that reached the disk, in order.
    pub fn writes(&self) -> Vec<(AbsPath, String)> {
        locked(&self.0).writes.clone()
    }
}

impl Files for FakeFiles {
    fn real(&self, path: &AbsPath) -> Result<AbsPath, FileFault> {
        let disk = locked(&self.0);
        for (at, to) in &disk.links {
            if at.covers(path) == Cover::Covers {
                let rest = &path.as_str()[at.as_str().len()..];
                let joined = format!("{}{rest}", to.as_str());
                return AbsPath::parse(&joined).map_err(|_| FileFault::Refused);
            }
        }
        Ok(path.clone())
    }

    fn read(&mut self, real: &AbsPath, _within: &AbsPath) -> Result<String, FileFault> {
        let mut disk = locked(&self.0);
        disk.reads += 1;
        disk.files.get(real).cloned().ok_or(FileFault::NotFound)
    }

    fn write(
        &mut self,
        real: &AbsPath,
        _within: &AbsPath,
        content: &str,
    ) -> Result<Option<String>, FileFault> {
        let mut disk = locked(&self.0);
        disk.writes.push((real.clone(), content.to_owned()));
        Ok(disk.files.insert(real.clone(), content.to_owned()))
    }

    fn remove(&mut self, real: &AbsPath, _within: &AbsPath) -> Result<(), FileFault> {
        locked(&self.0)
            .files
            .remove(real)
            .map(|_| ())
            .ok_or(FileFault::NotFound)
    }
}
