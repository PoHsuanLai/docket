//! A scripted fake ACP agent on the far end of an in-memory pipe. It answers `initialize` and
//! `session/new` itself; on each `session/prompt` it plays that turn's script (notifications,
//! requests to the client whose replies it records, and the final stop reason). No process, no
//! network, no clock.

use docket_acp::Wire;
use docket_acp::client::fake::{ChannelWire, pipe};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

pub const AGENT_SESSION: &str = "agent-s1";

pub type Replies = BTreeMap<String, Result<Value, Value>>;
pub type Make = Box<dyn Fn(&Replies) -> Value + Send>;

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
    Bridge {
        tag: &'static str,
        make: Box<dyn Fn(&Offered) -> (String, Value) + Send>,
    },
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
    /// Every line the agent wrote.
    pub sent: Vec<Value>,
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

    pub fn cancels(&self) -> usize {
        locked(&self.0).cancels
    }
}

async fn put(wire: &mut ChannelWire, view: &View, line: String) {
    if let Ok(value) = serde_json::from_str::<Value>(&line) {
        locked(&view.0).sent.push(value);
    }
    let _ = wire.write_line(line).await;
}

fn reply(id: &Value, result: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
}

async fn next(wire: &mut ChannelWire, view: &View) -> Option<Value> {
    loop {
        let line = wire.read_line().await?;
        let value: Value = serde_json::from_str(&line).expect("the client sends JSON");
        locked(&view.0).lines.push(value.clone());
        if value["method"] == "session/cancel" {
            locked(&view.0).cancels += 1;
            continue;
        }
        return Some(value);
    }
}

/// Reads until the reply to `id`, records it under `tag`. False when the pipe closed first.
async fn collect(wire: &mut ChannelWire, view: &View, id: &Value, tag: &'static str) -> bool {
    loop {
        let Some(got) = next(wire, view).await else {
            return false;
        };
        if got["id"] == *id && got.get("method").is_none() {
            let outcome = match got.get("error") {
                Some(e) => Err(e.clone()),
                None => Ok(got["result"].clone()),
            };
            locked(&view.0).replies.insert(tag.to_owned(), outcome);
            return true;
        }
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

/// Plays `acts`; true when the agent exited.
async fn play(
    wire: &mut ChannelWire,
    view: &View,
    prompt_id: &Value,
    acts: Vec<Act>,
    offered: &Option<Offered>,
) -> bool {
    let mut n = 0_u64;
    let mut fired: BTreeMap<&str, Value> = BTreeMap::new();
    for act in acts {
        match act {
            Act::Update(update) => {
                let note = json!({
                    "jsonrpc": "2.0", "method": "session/update",
                    "params": {"sessionId": AGENT_SESSION, "update": update}
                });
                put(wire, view, note.to_string()).await;
            }
            Act::Call {
                tag,
                method,
                params,
            } => {
                n += 1;
                let id = json!(format!("a{n}"));
                let params = params(&locked(&view.0).replies);
                let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
                put(wire, view, line.to_string()).await;
                if !collect(wire, view, &id, tag).await {
                    return true;
                }
            }
            Act::Fire {
                tag,
                method,
                params,
            } => {
                n += 1;
                let id = json!(format!("a{n}"));
                let params = params(&locked(&view.0).replies);
                let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
                put(wire, view, line.to_string()).await;
                fired.insert(tag, id);
            }
            Act::Collect(tag) => {
                let id = fired.remove(tag).expect("fired before collected");
                if !collect(wire, view, &id, tag).await {
                    return true;
                }
            }
            Act::Stop(reason) => {
                put(wire, view, reply(prompt_id, json!({"stopReason": reason}))).await;
            }
            Act::Exit => return true,
            Act::Bridge { tag, make } => {
                let outcome = match offered {
                    Some(offered) => {
                        let (socket, line) = make(offered);
                        over_socket(&socket, &line).await
                    }
                    None => Err(json!("no server was offered")),
                };
                locked(&view.0).replies.insert(tag.to_owned(), outcome);
            }
        }
    }
    false
}

async fn run(mut wire: ChannelWire, view: View, mut turns: std::collections::VecDeque<Vec<Act>>) {
    let mut offered = None;
    while let Some(msg) = next(&mut wire, &view).await {
        let id = msg["id"].clone();
        match msg["method"].as_str() {
            Some("initialize") => {
                locked(&view.0).client_caps = Some(msg["params"]["clientCapabilities"].clone());
                let result = json!({"protocolVersion": 1, "agentCapabilities": {}});
                put(&mut wire, &view, reply(&id, result)).await;
            }
            Some("session/new") => {
                locked(&view.0).new_session = Some(msg["params"].clone());
                offered = Offered::from_params(&msg["params"]);
                let result = json!({"sessionId": AGENT_SESSION});
                put(&mut wire, &view, reply(&id, result)).await;
            }
            Some("session/prompt") => {
                locked(&view.0).prompts.push(msg["params"].clone());
                let acts = turns
                    .pop_front()
                    .unwrap_or_else(|| vec![Act::Stop("end_turn")]);
                if play(&mut wire, &view, &id, acts, &offered).await {
                    return;
                }
            }
            _ => {}
        }
    }
}

/// An agent playing `turns`, one script per prompt, and the client's end of the pipe.
pub fn agent(turns: Vec<Vec<Act>>) -> (ChannelWire, View) {
    let (client, far) = pipe();
    let view = View::default();
    tokio::spawn(run(far, view.clone(), turns.into()));
    (client, view)
}

/// The parameters of a `fs/read_text_file` for `path` in the agent's session.
pub fn read(path: &str) -> Value {
    json!({"sessionId": AGENT_SESSION, "path": path})
}
