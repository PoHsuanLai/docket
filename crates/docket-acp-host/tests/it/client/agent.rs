//! A scripted fake ACP agent on the far end of an in-memory pipe. It answers `initialize` and
//! `session/new` itself; on each `session/prompt` it plays that turn's script (notifications,
//! requests to the client whose replies it records, and the final stop reason). No process, no
//! network, no clock.

use docket_acp_host::fake::{ChannelWire, pipe};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

#[path = "agent/play.rs"]
mod play;

pub const AGENT_SESSION: &str = "agent-s1";

pub type Replies = BTreeMap<String, Result<Value, Value>>;
pub type Make = Box<dyn Fn(&Replies) -> Value + Send>;
/// What a forged bridge line is built from the offered entry: the socket and the line.
pub type Forge = Box<dyn Fn(&Offered) -> (String, Value) + Send>;

/// How the fake agent treats sign-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Auth {
    /// Advertises no methods and opens a session for anyone.
    #[default]
    Open,
    /// Advertises `oauth-personal` (the agent signs itself in), `gemini-api-key` (the same) and
    /// a terminal method it cannot take through `authenticate`. `session/new` fails with
    /// "authentication required" until `oauth-personal` is authenticated, and also when it
    /// arrives before the reply to `authenticate` has been read by the client.
    Needed,
}

/// How the fake agent offers models in its `session/new` reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Offer {
    /// No choice of model.
    #[default]
    None,
    /// A session option in the model category (`flash` now; `flash`, `gemini-pro-agent`).
    Option,
    /// The older model list with the same models.
    Older,
    /// The option, but the agent refuses to switch.
    Stubborn,
}

/// Whether the fake agent can pick a session of its own up again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Resumes {
    /// Advertises nothing: `session/resume` is never sent to it.
    #[default]
    No,
    /// Advertises `session/resume` and answers it for any session.
    Yes,
}

/// One thing the agent does during a prompt.
pub enum Act {
    /// Sends a `session/update` notification with this update.
    Update(Value),
    /// Sends a request to the client, records the reply under `tag`.
    Call {
        tag: &'static str,
        method: &'static str,
        params: Make,
    },
    /// Sends a request and goes on without waiting for its reply.
    Fire {
        tag: &'static str,
        method: &'static str,
        params: Make,
    },
    /// Waits for the reply to a fired request and records it.
    Collect(&'static str),
    /// Answers the prompt with this stop reason. Always last.
    Stop(&'static str),
    /// The process ends: the pipe closes.
    Exit,
    /// What the MCP server entry the agent was offered would do: connects to a socket, sends one
    /// line, records the parsed reply under `tag` (`Err("unreachable")` when nothing answers).
    /// `make` is given the entry as offered and may forge anything the agent could write.
    Bridge { tag: &'static str, make: Forge },
}

/// The MCP server entry the agent was offered in `session/new`, as it read it.
#[derive(Debug, Clone, Default)]
pub struct Offered {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

impl Offered {
    pub fn from_params(params: &Value) -> Option<Self> {
        let server = params["mcpServers"].get(0)?;
        let strings = |v: &Value| -> Vec<String> {
            v.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        };
        Some(Self {
            name: server["name"].as_str()?.to_owned(),
            command: server["command"].as_str()?.to_owned(),
            args: strings(&server["args"]),
            env: server["env"]
                .as_array()?
                .iter()
                .filter_map(|e| {
                    Some((
                        e["name"].as_str()?.to_owned(),
                        e["value"].as_str()?.to_owned(),
                    ))
                })
                .collect(),
        })
    }

    /// The socket the bridge program is told to connect to (the argument after the flag).
    pub fn socket(&self) -> String {
        self.args.get(1).cloned().unwrap_or_default()
    }

    /// The token in the bridge's environment.
    pub fn token(&self) -> String {
        self.env
            .get("QUIRE_EDGE_TOKEN")
            .cloned()
            .unwrap_or_default()
    }

    /// A request line as the bridge would send it: the right token, this operation.
    pub fn line(&self, op: Value) -> (String, Value) {
        (self.socket(), json!({"token": self.token(), "op": op}))
    }
}

/// The agent lists the tools through its bridge.
pub fn bridge_list(tag: &'static str) -> Act {
    Act::Bridge {
        tag,
        make: Box::new(|o| o.line(json!("list"))),
    }
}

/// The agent calls a tool through its bridge, as offered.
pub fn bridge_call(tag: &'static str, tool: &'static str, arguments: Value) -> Act {
    Act::Bridge {
        tag,
        make: Box::new(move |o| {
            o.line(json!({"call": {"tool": tool, "arguments": arguments.clone()}}))
        }),
    }
}

pub fn call(tag: &'static str, method: &'static str, params: Value) -> Act {
    Act::Call {
        tag,
        method,
        params: Box::new(move |_| params.clone()),
    }
}

pub fn call_with(
    tag: &'static str,
    method: &'static str,
    make: impl Fn(&Replies) -> Value + Send + 'static,
) -> Act {
    Act::Call {
        tag,
        method,
        params: Box::new(make),
    }
}

pub fn fire_with(
    tag: &'static str,
    method: &'static str,
    make: impl Fn(&Replies) -> Value + Send + 'static,
) -> Act {
    Act::Fire {
        tag,
        method,
        params: Box::new(make),
    }
}

pub fn say(text: &str) -> Act {
    Act::Update(json!({
        "sessionUpdate": "agent_message_chunk",
        "content": {"type": "text", "text": text}
    }))
}

pub fn think(text: &str) -> Act {
    Act::Update(json!({
        "sessionUpdate": "agent_thought_chunk",
        "content": {"type": "text", "text": text}
    }))
}

/// What the agent saw and the client sent it.
#[derive(Debug, Default)]
pub struct Seen {
    pub replies: Replies,
    pub lines: Vec<Value>,
    pub client_caps: Option<Value>,
    pub new_session: Option<Value>,
    pub prompts: Vec<Value>,
    pub cancels: usize,
    /// The `authenticate` parameters, in order.
    pub authenticated: Vec<Value>,
    /// `session/new` lines that were already sent before the `authenticate` reply was read.
    pub raced: usize,
    /// Every line the agent wrote.
    pub sent: Vec<Value>,
    /// Model switches asked for: the method and its parameters, in order.
    pub switches: Vec<(String, Value)>,
    /// The `session/resume` parameters, in order.
    pub resumed: Vec<Value>,
}

#[derive(Clone, Default)]
pub struct View(Arc<Mutex<Seen>>);

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl View {
    pub fn reply(&self, tag: &str) -> Result<Value, Value> {
        locked(&self.0)
            .replies
            .get(tag)
            .cloned()
            .unwrap_or_else(|| panic!("the agent has no reply for {tag}"))
    }

    pub fn lines(&self) -> Vec<Value> {
        locked(&self.0).lines.clone()
    }

    pub fn client_caps(&self) -> Value {
        locked(&self.0)
            .client_caps
            .clone()
            .expect("initialize seen")
    }

    pub fn new_session(&self) -> Value {
        locked(&self.0)
            .new_session
            .clone()
            .expect("session/new seen")
    }

    pub fn prompts(&self) -> Vec<Value> {
        locked(&self.0).prompts.clone()
    }

    pub fn sent(&self) -> Vec<Value> {
        locked(&self.0).sent.clone()
    }

    pub fn authenticated(&self) -> Vec<Value> {
        locked(&self.0).authenticated.clone()
    }

    pub fn switches(&self) -> Vec<(String, Value)> {
        locked(&self.0).switches.clone()
    }

    pub fn resumed(&self) -> Vec<Value> {
        locked(&self.0).resumed.clone()
    }

    pub fn raced(&self) -> usize {
        locked(&self.0).raced
    }

    pub fn cancels(&self) -> usize {
        locked(&self.0).cancels
    }
}

/// Sends `line` to the unix socket `path` and reads one line back.
pub async fn over_socket(path: &str, line: &Value) -> Result<Value, Value> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let unreachable = |_| json!("unreachable");
    let stream = tokio::net::UnixStream::connect(path)
        .await
        .map_err(unreachable)?;
    let (read, mut write) = stream.into_split();
    let text = format!("{line}\n");
    write
        .write_all(text.as_bytes())
        .await
        .map_err(unreachable)?;
    let mut back = String::new();
    BufReader::new(read)
        .read_line(&mut back)
        .await
        .map_err(unreachable)?;
    serde_json::from_str(back.trim()).map_err(|_| json!("unreachable"))
}

/// An agent playing `turns`, one script per prompt, and the client's end of the pipe.
pub fn agent(turns: Vec<Vec<Act>>) -> (ChannelWire, View) {
    agent_signing(turns, Auth::Open)
}

/// The same, with the sign-in it takes.
pub fn agent_signing(turns: Vec<Vec<Act>>, auth: Auth) -> (ChannelWire, View) {
    agent_offering(turns, auth, Offer::None)
}

/// The same, with the models it offers.
pub fn agent_offering(turns: Vec<Vec<Act>>, auth: Auth, offer: Offer) -> (ChannelWire, View) {
    agent_resuming(turns, (auth, offer), Resumes::No)
}

/// The same, with whether it can pick a session of its own up again.
pub fn agent_resuming(
    turns: Vec<Vec<Act>>,
    (auth, offer): (Auth, Offer),
    resumes: Resumes,
) -> (ChannelWire, View) {
    docket_testbus::hang_guard::arm();
    let (client, far) = pipe();
    let view = View::default();
    tokio::spawn(play::run(
        far,
        view.clone(),
        turns.into(),
        (auth, offer, resumes),
    ));
    (client, view)
}

/// The parameters of a `fs/read_text_file` for `path` in the agent's session.
pub fn read(path: &str) -> Value {
    json!({"sessionId": AGENT_SESSION, "path": path})
}
