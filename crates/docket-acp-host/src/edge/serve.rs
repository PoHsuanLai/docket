//! The listening side of one session's tool edge.

use super::menu::EdgeMenu;
use super::offer::{EdgeBind, ToolsOffer};
use super::token::{mint, nonce};
use acp_core::call::AgentCall;
use acp_core::court::{Court, Ruled};
use actions_tools::{
    EDGE_LINE_MAX, EdgeOp, EdgeReply, EdgeRequest, EdgeToken, EdgeTool, ToolFault, ToolRefusal,
    listing, outcome_json, prepare,
};
use docket_core::ProgramName;
use docket_core::{AbsPath, BreakerTrip, CallRefusal};
use docket_procroot::own_uid;

use prov::{ClientName, SessionId};
use serde_json::Value as Json;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::task::{JoinHandle, JoinSet};

/// The first number an edge call takes in the court's table of calls in flight, far above the
/// backend's own, which count up from nothing in this connection.
const FIRST_CALL: u64 = 1 << 40;

/// Why an edge could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EdgeFault {
    /// The socket directory, the socket or the token could not be made.
    #[error("the tool edge could not be made")]
    Io,
    /// A path is not one the sandbox's binds can name.
    #[error("the tool edge's paths are not absolute text")]
    Path,
}

/// The tool edge of one session. Dropping it stops it, ends every connection and removes the
/// socket, so a call after the session is closed finds nothing to talk to.
#[derive(Debug)]
pub struct ToolsEdge {
    token: EdgeToken,
    bind: EdgeBind,
    dir: PathBuf,
    tripped: Tripped,
    task: JoinHandle<()>,
}

/// Where a connection leaves word that the router's breaker paused the session, for the backend
/// to read: a pause seen by a call over the edge ends the turn as one seen by a file call does.
type Tripped = Arc<Mutex<Option<BreakerTrip>>>;

/// What a connection's handler needs, all fixed when the edge was made.
struct Serving<C> {
    court: C,
    session: SessionId,
    token: EdgeToken,
    client: ClientName,
    owner: u32,
    next: Arc<AtomicU64>,
    tripped: Tripped,
    menu: EdgeMenu,
}

impl<C: Clone> Clone for Serving<C> {
    fn clone(&self) -> Self {
        Self {
            court: self.court.clone(),
            session: self.session.clone(),
            token: self.token.clone(),
            client: self.client.clone(),
            owner: self.owner,
            next: self.next.clone(),
            tripped: self.tripped.clone(),
            menu: self.menu.clone(),
        }
    }
}

fn private_dir(run_dir: &std::path::Path) -> std::io::Result<PathBuf> {
    let dir = run_dir.join(format!("docket-edge-{}", nonce()?));
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    Ok(dir)
}

fn abs(path: &std::path::Path) -> Result<AbsPath, EdgeFault> {
    path.to_str()
        .and_then(|t| AbsPath::parse(t).ok())
        .ok_or(EdgeFault::Path)
}

fn client_of(program: &ProgramName) -> ClientName {
    ClientName::parse(&format!("acp:{}", program.as_str()))
        .or_else(|_| ClientName::parse("acp-agent"))
        .expect("`acp-agent` is a valid client name")
}

impl ToolsEdge {
    /// Makes the edge of `session`, for the agent of `program`, whose calls go to the router
    /// through `court`. Must run inside the tokio runtime.
    pub fn start<C: Court>(
        offer: &ToolsOffer,
        session: &SessionId,
        program: &ProgramName,
        court: C,
    ) -> Result<Self, EdgeFault> {
        Self::start_with(offer, session, program, court, EdgeMenu::Everything)
    }

    /// [`Self::start`], offering only what `menu` allows.
    pub fn start_with<C: Court>(
        offer: &ToolsOffer,
        session: &SessionId,
        program: &ProgramName,
        court: C,
        menu: EdgeMenu,
    ) -> Result<Self, EdgeFault> {
        let io = |_| EdgeFault::Io;
        let owner = own_uid().ok_or(EdgeFault::Io)?;
        let token = mint().map_err(io)?;
        let dir = private_dir(&offer.run_dir).map_err(io)?;
        let socket = dir.join("tools.sock");
        let made = std::os::unix::net::UnixListener::bind(&socket)
            .and_then(|l| l.set_nonblocking(true).map(|()| l))
            .and_then(UnixListener::from_std);
        let listener = match made {
            Ok(listener) => listener,
            Err(_) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(EdgeFault::Io);
            }
        };
        let bind = EdgeBind {
            socket: abs(&socket)?,
            bridge: offer.bridge.clone(),
        };
        let tripped = Tripped::default();
        let serving = Serving {
            court,
            session: session.clone(),
            token: token.clone(),
            client: client_of(program),
            owner,
            next: Arc::new(AtomicU64::new(FIRST_CALL)),
            tripped: tripped.clone(),
            menu,
        };
        let task = tokio::spawn(accept(listener, serving));
        Ok(Self {
            token,
            bind,
            dir,
            tripped,
            task,
        })
    }

    /// What the launcher binds into the sandbox.
    pub fn bind(&self) -> &EdgeBind {
        &self.bind
    }

    /// The breaker's trip a call over the edge met, once: the backend ends the turn on it.
    pub fn take_trip(&self) -> Option<BreakerTrip> {
        self.tripped
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    /// The token, for the environment of the MCP server entry.
    pub fn token(&self) -> &EdgeToken {
        &self.token
    }
}

impl Drop for ToolsEdge {
    fn drop(&mut self) {
        self.task.abort();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn accept<C: Court>(listener: UnixListener, serving: Serving<C>) {
    // Connections live in this set, so ending this task ends them all.
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { return };
                connections.spawn(connection(stream, serving.clone()));
            }
            Some(_) = connections.join_next() => {}
        }
    }
}

/// One line, at most `EDGE_LINE_MAX` bytes; none if the stream ended or the line was longer.
async fn line(reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    let read = (&mut *reader)
        .take(EDGE_LINE_MAX as u64)
        .read_until(b'\n', &mut buf)
        .await
        .ok()?;
    (read > 0 && buf.ends_with(b"\n")).then_some(buf)
}

async fn connection<C: Court>(stream: UnixStream, mut serving: Serving<C>) {
    // Only the user that runs the host may speak here, whatever the directory's mode.
    if stream.peer_cred().map(|c| c.uid()).ok() != Some(serving.owner) {
        return;
    }
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    while let Some(bytes) = line(&mut reader).await {
        let reply = serving.answer(&bytes).await;
        let Ok(mut text) = serde_json::to_string(&reply) else {
            return;
        };
        text.push('\n');
        if write.write_all(text.as_bytes()).await.is_err() {
            return;
        }
    }
}

impl<C: Court> Serving<C> {
    /// The reply to one line. A line that is not a request, or whose token is not this
    /// session's, gets `Refused` and nothing else happens.
    async fn answer(&mut self, bytes: &[u8]) -> EdgeReply {
        let Ok(request) = serde_json::from_slice::<EdgeRequest>(bytes) else {
            return EdgeReply::Refused;
        };
        if !self.token.matches(&request.token) {
            return EdgeReply::Refused;
        }
        let Some(registry) = self.court.registry().await else {
            return EdgeReply::Failed(ToolFault::Unavailable);
        };
        match request.op {
            EdgeOp::List => EdgeReply::Tools(
                listing(&registry, |action| self.menu.offers(action))
                    .into_iter()
                    .map(|(tool, description)| EdgeTool { tool, description })
                    .collect(),
            ),
            EdgeOp::Call { tool, arguments } => {
                match self.call(&registry, &tool, arguments).await {
                    Ok(done) => EdgeReply::Done(done),
                    Err(fault) => EdgeReply::Failed(fault),
                }
            }
        }
    }

    async fn call(
        &mut self,
        registry: &[docket_core::ValidManifest],
        tool: &str,
        arguments: Json,
    ) -> Result<Json, ToolFault> {
        let request = prepare(registry, tool, &arguments, &self.client, |action| {
            self.menu.offers(action)
        })?;
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        let call = AgentCall::Tool(Box::new(request));
        match self.court.call(&self.session, n, &call).await {
            Ruled::Done(outcome) => Ok(outcome_json(&outcome)),
            Ruled::Refused(refusal) => {
                if let CallRefusal::Paused(trip) = &refusal {
                    *self.tripped.lock().unwrap_or_else(|e| e.into_inner()) = Some(*trip);
                }
                Err(ToolRefusal::from(&refusal).into())
            }
            Ruled::Lost => Err(ToolFault::Unavailable),
        }
    }
}
