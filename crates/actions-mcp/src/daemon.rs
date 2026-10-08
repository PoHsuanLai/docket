//! The binary's body: the session bus, the bus name the `mcp` role is written under, and one
//! edge per client connection, over stdio or a Unix socket.

use crate::config::{ConfigError, McpConfig};
use crate::edge::McpEdge;
use crate::expose::McpExpose;
use docket_client::{DbusTransport, Intents};
use docket_dbus::BusConnection;
use rmcp::ServiceExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use zbus::fdo::RequestNameFlags;
use zbus::fdo::RequestNameReply;

/// The bus name the shipped `intentd.toml` gives the `mcp` role.
pub const MCP_BUS: &str = "org.quire.ActionsMcp";

/// Where the edge speaks MCP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listen {
    /// The process's own standard input and output, for one client that started it.
    Stdio,
    /// A Unix socket that many clients connect to in turn.
    Socket(PathBuf),
}

/// Why the edge stopped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DaemonFault {
    /// The configuration file does not read.
    #[error("{0}")]
    Config(#[from] ConfigError),
    /// The bus name is taken or the connection failed.
    #[error("bus: {0}")]
    Bus(String),
    /// The socket or the stream failed.
    #[error("io: {0}")]
    Io(String),
    /// A bridge was started without its session's token.
    #[error("{0}")]
    NoToken(#[from] crate::bound::NoToken),
}

fn bus(error: zbus::Error) -> DaemonFault {
    DaemonFault::Bus(error.to_string())
}

fn io(error: impl std::fmt::Display) -> DaemonFault {
    DaemonFault::Io(error.to_string())
}

/// Takes the `mcp` role's bus name. intentd derives a caller's roles from the names its connection
/// owns, so this process is `Actor::Mcp` only while it holds it; a second edge finds it taken and
/// stops (several clients share one edge through the socket).
pub async fn claim(connection: &BusConnection) -> Result<(), DaemonFault> {
    let reply = connection
        .request_name_with_flags(MCP_BUS, RequestNameFlags::DoNotQueue.into())
        .await
        .map_err(bus)?;
    match reply {
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => Ok(()),
        RequestNameReply::InQueue | RequestNameReply::Exists => {
            Err(DaemonFault::Bus(format!("{MCP_BUS} is taken")))
        }
    }
}

/// Tells whether the edge is on, asked at every request.
pub type ExposeRead = Arc<dyn Fn() -> McpExpose + Send + Sync>;

fn edge(
    connection: &BusConnection,
    config: &McpConfig,
    expose: &ExposeRead,
) -> McpEdge<DbusTransport> {
    let expose = expose.clone();
    McpEdge::new(
        Intents::over(DbusTransport::new(connection.clone())),
        config.client.clone(),
    )
    .with_expose_read(move || expose())
}

async fn over_stdio<S: rmcp::ServerHandler>(edge: S) -> Result<(), DaemonFault> {
    let running = edge
        .serve((tokio::io::stdin(), tokio::io::stdout()))
        .await
        .map_err(io)?;
    running.waiting().await.map(|_| ()).map_err(io)
}

async fn over_socket(
    connection: BusConnection,
    config: McpConfig,
    expose: ExposeRead,
    path: &Path,
) -> Result<(), DaemonFault> {
    // A stale file from a crashed edge is ours to replace; a live one would refuse the bind below
    // only after this removes it, so the bus name (taken first) is what keeps two edges apart.
    let _ = std::fs::remove_file(path);
    let listener = tokio::net::UnixListener::bind(path).map_err(io)?;
    eprintln!("actions-mcp: listening on {}", path.display());
    loop {
        let (stream, _) = listener.accept().await.map_err(io)?;
        let edge = edge(&connection, &config, &expose);
        tokio::spawn(async move {
            if let Ok(running) = edge.serve(stream).await {
                let _ = running.waiting().await;
            }
        });
    }
}

/// Serves the edge on `listen` over `connection` until the client goes (stdio) or forever (socket),
/// switched as `config.expose` says for as long as it runs.
pub async fn start(
    connection: &BusConnection,
    config: McpConfig,
    listen: Listen,
) -> Result<(), DaemonFault> {
    let expose = config.expose;
    start_following(connection, config, listen, Arc::new(move || expose)).await
}

/// `start`, with the switch asked of `expose` at every request: the daemon hands it the person's
/// settings file, read again each time.
pub async fn start_following(
    connection: &BusConnection,
    config: McpConfig,
    listen: Listen,
    expose: ExposeRead,
) -> Result<(), DaemonFault> {
    claim(connection).await?;
    match listen {
        Listen::Stdio => over_stdio(edge(connection, &config, &expose)).await,
        Listen::Socket(path) => over_socket(connection.clone(), config, expose, &path).await,
    }
}

/// What the command line asks for: `--socket PATH`, `--client NAME`, `--write-schema DIR`, `--host-socket PATH`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Args {
    /// Serve a socket instead of stdio.
    pub socket: Option<PathBuf>,
    /// The client name, over the configuration's.
    pub client: Option<prov::ClientName>,
    /// Write docket's settings schema into this directory and stop.
    pub write_schema: Option<PathBuf>,
    /// Be the bridge of an external agent's session: forward to the host's socket on stdio. The
    /// token is in the environment, never here.
    pub host_socket: Option<PathBuf>,
}

impl Args {
    /// Reads the arguments after the program name; an unknown one is an error.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut args = args.into_iter();
        let mut parsed = Self::default();
        while let Some(flag) = args.next() {
            let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
            match flag.as_str() {
                "--socket" => parsed.socket = Some(PathBuf::from(value()?)),
                "--write-schema" => parsed.write_schema = Some(PathBuf::from(value()?)),
                "--host-socket" => parsed.host_socket = Some(PathBuf::from(value()?)),
                "--client" => {
                    parsed.client = Some(
                        prov::ClientName::parse(&value()?)
                            .map_err(|_| "--client: not a client name".to_owned())?,
                    )
                }
                other => return Err(format!("unknown argument {other}")),
            }
        }
        Ok(parsed)
    }
}

/// The daemon: the configuration and the session bus from the environment, the arguments from the
/// command line.
pub async fn run(args: Args) -> Result<(), DaemonFault> {
    if let Some(dir) = args.write_schema {
        return write_schema(&dir);
    }
    let env = |key: &str| std::env::var(key).ok();
    if let Some(socket) = args.host_socket {
        let edge = crate::bound::BoundEdge::from_env(socket, &env)?;
        return over_stdio(edge).await;
    }
    let mut config = McpConfig::from_env(&env)?;
    if let Some(client) = args.client {
        config = config.named(client);
    }
    if config.expose == McpExpose::Off {
        eprintln!(
            "actions-mcp: off (set agent.mcp.expose = \"on\" in docket/settings.toml): no tools are listed"
        );
    }
    let connection = docket_dbus::session_connection(&env).await.map_err(bus)?;
    let listen = args.socket.map_or(Listen::Stdio, Listen::Socket);
    let locator = docket_settings::Locator::from_env(&env);
    let expose: ExposeRead = Arc::new(move || {
        locator
            .read(docket_settings::AgentSettings::default())
            .value
            .expose
    });
    start_following(&connection, config, listen, expose).await
}

/// `--write-schema DIR`: `docket.settings.toml` into `DIR`, for a local install (design/22 section
/// 9.2).
pub fn write_schema(dir: &Path) -> Result<(), DaemonFault> {
    std::fs::create_dir_all(dir).map_err(io)?;
    std::fs::write(dir.join("docket.settings.toml"), crate::settings::SCHEMA).map_err(io)
}
