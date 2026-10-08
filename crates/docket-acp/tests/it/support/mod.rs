//! A fake ACP client that plays the editor over an in-memory duplex stream, and the fixtures
//! around it. Scripted: no network, no process, no clock.
#![allow(dead_code)]

use agent_client_protocol_schema::rpc::RequestId;
use docket_acp::{LineWire, Permit, Server, Ticks};
use docket_core::{
    ActionRef, CallId, CallRefusal, DenyCode, LabelText, StepEnd, StepLine, StepShown,
};
use docket_session::fake::{FakeBackend, MemoryLog};
use docket_session::fake_host::FakeHost;
use docket_session::{BackendEvent, CallEvent, CallOpen, TurnEnd};
use porter_core::AppName;
use prov::{ActionName, Effect, UnixSeconds};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, duplex};
use tokio::io::{AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf, split};

pub type Wire = LineWire<BufReader<ReadHalf<DuplexStream>>, WriteHalf<DuplexStream>>;
pub type TestServer = Server<FakeHost, Arc<MemoryLog>, Wire, Fixed>;

/// A clock that stands still.
pub struct Fixed;
impl Ticks for Fixed {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(1_760_000_000)
    }
}

pub fn app(name: &str) -> AppName {
    AppName::parse(name).expect("app")
}

pub fn permit() -> Permit {
    Permit::from_text("[agent.acp]\nexpose = \"on\"\n").expect("on")
}

pub fn action(name: &str) -> ActionRef {
    ActionRef {
        app: app("org.quire.Mail"),
        name: ActionName::parse(name).expect("action"),
    }
}

pub fn open(n: u64, name: &str, effect: Effect) -> CallOpen {
    CallOpen {
        call: CallId(n),
        action: action(name),
        effect,
    }
}

pub fn started(n: u64, name: &str, effect: Effect) -> BackendEvent {
    BackendEvent::Call(CallEvent::Started(open(n, name, effect)))
}

pub fn step(n: u64, name: &str, effect: Effect, end: StepEnd) -> BackendEvent {
    BackendEvent::Call(CallEvent::Ended(StepLine {
        call: CallId(n),
        action: action(name),
        effect,
        end,
        shown: StepShown::Full,
        with: Vec::new(),
    }))
}

pub fn done(said: &str) -> StepEnd {
    StepEnd::Done {
        said: Some(LabelText::parse(said).expect("said")),
        value: None,
        undo: None,
    }
}

pub fn denied(code: DenyCode) -> StepEnd {
    StepEnd::Refused(CallRefusal::Denied(code))
}

pub fn words(text: &str) -> BackendEvent {
    BackendEvent::Words(docket_core::Reveal::Plain(text.to_owned()))
}

pub fn end(how: TurnEnd) -> BackendEvent {
    BackendEvent::TurnEnd(how)
}

/// The editor: the other end of the wire, with everything it saw.
pub struct Editor {
    to_server: WriteHalf<DuplexStream>,
    from_server: Lines<BufReader<ReadHalf<DuplexStream>>>,
    next_id: i64,
    pub seen: Vec<Value>,
    schema: Value,
}

/// A server for `editor_app` over `log` with `scripts`, and the editor wired to it.
pub fn rig(
    log: &Arc<MemoryLog>,
    scripts: Vec<Vec<Vec<BackendEvent>>>,
    editor_app: &str,
) -> (TestServer, Editor) {
    docket_testbus::hang_guard::arm();
    let (client_end, server_end) = duplex(1 << 16);
    let (server_read, server_write) = split(server_end);
    let (client_read, client_write) = split(client_end);
    let wire = LineWire::new(BufReader::new(server_read), server_write);
    let host = FakeHost::new(log.clone(), scripts);
    let server = Server::new(permit(), app(editor_app), host, log.clone(), wire, Fixed);
    let schema: Value =
        serde_json::from_str(include_str!("../../schema/schema.json")).expect("schema");
    let editor = Editor {
        to_server: client_write,
        from_server: BufReader::new(client_read).lines(),
        next_id: 0,
        seen: Vec::new(),
        schema,
    };
    (server, editor)
}

/// A scratch backend for hosts that never run a turn.
pub fn quiet() -> FakeBackend {
    FakeBackend::new(Vec::new())
}

impl Editor {
    /// `value` must fit the schema's definition `def`.
    pub fn conforms(&self, def: &str, value: &Value) {
        let root = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$ref": format!("#/$defs/{def}"),
            "$defs": self.schema["$defs"],
        });
        let validator = jsonschema::validator_for(&root).expect("validator");
        let errors: Vec<String> = validator
            .iter_errors(value)
            .map(|e| e.to_string())
            .collect();
        assert!(errors.is_empty(), "{def}: {errors:?} in {value}");
    }

    async fn write(&mut self, body: Value) {
        let mut line = body.to_string();
        line.push('\n');
        self.to_server
            .write_all(line.as_bytes())
            .await
            .expect("write");
    }

    pub async fn raw(&mut self, line: &str) {
        self.to_server
            .write_all(format!("{line}\n").as_bytes())
            .await
            .expect("write");
    }

    pub async fn notify(&mut self, method: &str, params: Value, def: &str) {
        self.conforms(def, &params);
        self.write(json!({"jsonrpc": "2.0", "method": method, "params": params}))
            .await;
    }

    /// Sends a request and returns its id.
    pub async fn send(&mut self, method: &str, params: Value, def: &str) -> i64 {
        self.conforms(def, &params);
        self.next_id += 1;
        let id = self.next_id;
        self.write(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        id
    }

    /// A request the schema has no definition for (a method we do not serve).
    pub async fn send_unchecked(&mut self, method: &str) -> i64 {
        self.next_id += 1;
        let id = self.next_id;
        self.write(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": {}}))
            .await;
        id
    }

    /// The next message from the server, validated against the schema as what it is.
    pub async fn recv(&mut self) -> Value {
        let line = self
            .from_server
            .next_line()
            .await
            .expect("read")
            .expect("a line");
        let message: Value = serde_json::from_str(&line).expect("json");
        self.seen.push(message.clone());
        if let Some(method) = message["method"].as_str() {
            match method {
                "session/update" => self.conforms("SessionNotification", &message["params"]),
                "session/request_permission" => {
                    self.conforms("RequestPermissionRequest", &message["params"]);
                }
                other => panic!("unexpected server method {other}"),
            }
        }
        message
    }

    /// Reads until the reply to `id`; each request the server makes meanwhile is answered with
    /// `answer(request)`. Returns the reply message.
    pub async fn until_reply(
        &mut self,
        id: i64,
        def: &str,
        answer: &mut dyn FnMut(&Value) -> Option<Value>,
    ) -> Value {
        loop {
            let message = self.recv().await;
            if message["method"] == "session/request_permission" {
                if let Some(result) = answer(&message) {
                    self.conforms("RequestPermissionResponse", &result);
                    self.write(json!({"jsonrpc": "2.0", "id": message["id"], "result": result}))
                        .await;
                }
                continue;
            }
            if message["id"] == json!(id) {
                if message.get("result").is_some() {
                    self.conforms(def, &message["result"]);
                }
                return message;
            }
        }
    }

    /// A request answered at once (no permission request expected).
    pub async fn call(&mut self, method: &str, params: Value, req: &str, resp: &str) -> Value {
        let id = self.send(method, params, req).await;
        self.until_reply(id, resp, &mut |_| None).await
    }

    pub async fn initialize(&mut self) -> Value {
        let params = json!({"protocolVersion": 1, "clientCapabilities": {}, "clientInfo": {"name": "zed", "version": "1"}});
        self.call(
            "initialize",
            params,
            "InitializeRequest",
            "InitializeResponse",
        )
        .await
    }

    pub async fn new_session(&mut self, cwd: &str) -> String {
        let reply = self
            .call(
                "session/new",
                json!({"cwd": cwd, "mcpServers": []}),
                "NewSessionRequest",
                "NewSessionResponse",
            )
            .await;
        reply["result"]["sessionId"]
            .as_str()
            .expect("session id")
            .to_owned()
    }

    /// Sends a prompt and reads to its reply, answering permission requests with `answer`.
    pub async fn prompt(
        &mut self,
        session: &str,
        text: &str,
        answer: &mut dyn FnMut(&Value) -> Option<Value>,
    ) -> Value {
        let params = json!({"sessionId": session, "prompt": [{"type": "text", "text": text}]});
        let id = self.send("session/prompt", params, "PromptRequest").await;
        self.until_reply(id, "PromptResponse", answer).await
    }

    /// Every `session/update` seen, as its `update` object.
    pub fn updates(&self) -> Vec<Value> {
        self.seen
            .iter()
            .filter(|m| m["method"] == "session/update")
            .map(|m| m["params"]["update"].clone())
            .collect()
    }

    pub fn permission_requests(&self) -> Vec<Value> {
        self.seen
            .iter()
            .filter(|m| m["method"] == "session/request_permission")
            .cloned()
            .collect()
    }

    /// The transcript, one message per line.
    pub fn transcript(&self) -> String {
        self.seen.iter().map(|m| format!("{m}\n")).collect()
    }
}

pub fn allow(request: &Value) -> Option<Value> {
    let _ = request;
    Some(json!({"outcome": {"outcome": "selected", "optionId": "allow_once"}}))
}

pub fn reject(request: &Value) -> Option<Value> {
    let _ = request;
    Some(json!({"outcome": {"outcome": "selected", "optionId": "reject_once"}}))
}

pub fn id_of(value: &Value) -> RequestId {
    serde_json::from_value(value.clone()).expect("id")
}
