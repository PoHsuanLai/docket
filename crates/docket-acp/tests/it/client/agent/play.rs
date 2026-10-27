//! The scripted agent's side of the pipe: it answers the handshake itself and plays each turn's
//! script when the prompt comes.

use super::{AGENT_SESSION, Act, Auth, Offer, Offered, Resumes, View, locked};
use docket_acp::client::fake::ChannelWire;
use serde_json::{Value, json};
use std::collections::BTreeMap;

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
                        super::over_socket(&socket, &line).await
                    }
                    None => Err(json!("no server was offered")),
                };
                locked(&view.0).replies.insert(tag.to_owned(), outcome);
            }
        }
    }
    false
}

const METHODS: &str = "oauth-personal";

fn models_part(offer: Offer) -> Value {
    let list = json!([
        {"value": "flash", "name": "Gemini Flash"},
        {"value": "gemini-pro-agent", "name": "Gemini 3.1 Pro"},
    ]);
    match offer {
        Offer::None => json!({}),
        Offer::Option | Offer::Stubborn => json!({"configOptions": [
            {"id": "mode", "name": "Mode", "category": "mode", "type": "select",
             "currentValue": "a", "options": [{"value": "a", "name": "A"}]},
            {"id": "model", "name": "Model", "category": "model", "type": "select",
             "currentValue": "flash", "options": list},
        ]}),
        Offer::Older => json!({"models": {
            "currentModelId": "flash",
            "availableModels": [
                {"modelId": "flash", "name": "Gemini Flash"},
                {"modelId": "gemini-pro-agent", "name": "Gemini 3.1 Pro"},
            ],
        }}),
    }
}

fn advertised(auth: Auth) -> Value {
    match auth {
        Auth::Open => json!([]),
        Auth::Needed => json!([
            {"id": METHODS, "name": "Personal sign-in"},
            {"id": "gemini-api-key", "name": "API key"},
            {"type": "terminal", "id": "login-terminal", "name": "Terminal sign-in",
             "args": ["--login"], "env": {"BROWSER": "none"}},
        ]),
    }
}

/// A line the client has already sent, if one is waiting right now. Nothing is awaited.
fn waiting(wire: &mut ChannelWire) -> Option<String> {
    use std::task::{Context, Poll, Waker};
    let mut read = std::pin::pin!(wire.read_line());
    match read.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(line) => line,
        Poll::Pending => None,
    }
}

fn refused_sign_in(id: &Value) -> String {
    let error = json!({
        "code": -32000,
        "message": "Authentication required",
        "data": {"message": "No authentication method selected."},
    });
    json!({"jsonrpc": "2.0", "id": id, "error": error}).to_string()
}

pub(super) async fn run(
    mut wire: ChannelWire,
    view: View,
    mut turns: std::collections::VecDeque<Vec<Act>>,
    (auth, offer, resumes): (Auth, Offer, Resumes),
) {
    let mut offered = None;
    let mut signed_in = auth == Auth::Open;
    while let Some(msg) = next(&mut wire, &view).await {
        let id = msg["id"].clone();
        match msg["method"].as_str() {
            Some("initialize") => {
                locked(&view.0).client_caps = Some(msg["params"]["clientCapabilities"].clone());
                // The agent calls itself something grand: that is its claim, and no label of ours.
                let result = json!({
                    "protocolVersion": 1,
                    "agentCapabilities": match resumes {
                        Resumes::No => json!({}),
                        Resumes::Yes => json!({"sessionCapabilities": {"resume": {}}}),
                    },
                    "authMethods": advertised(auth),
                    "agentInfo": {"name": "grand", "title": "The Desktop Itself", "version": "1"},
                });
                put(&mut wire, &view, reply(&id, result)).await;
            }
            Some("authenticate") => {
                locked(&view.0).authenticated.push(msg["params"].clone());
                if msg["params"]["methodId"] != METHODS {
                    let error = json!({"code": -32602, "message": "unknown method"});
                    let line = json!({"jsonrpc": "2.0", "id": id, "error": error});
                    put(&mut wire, &view, line.to_string()).await;
                    continue;
                }
                signed_in = true;
                // A client that sends `session/new` without waiting for this reply has it in
                // the pipe by now: it is answered as the real server does, with a refusal.
                if let Some(early) = waiting(&mut wire) {
                    let early: Value = serde_json::from_str(&early).expect("JSON");
                    locked(&view.0).lines.push(early.clone());
                    locked(&view.0).raced += 1;
                    put(&mut wire, &view, refused_sign_in(&early["id"])).await;
                }
                put(&mut wire, &view, reply(&id, json!({}))).await;
            }
            Some("session/new") if !signed_in => {
                put(&mut wire, &view, refused_sign_in(&id)).await;
            }
            Some("session/new") => {
                locked(&view.0).new_session = Some(msg["params"].clone());
                offered = Offered::from_params(&msg["params"]);
                let mut result = models_part(offer);
                result["sessionId"] = json!(AGENT_SESSION);
                put(&mut wire, &view, reply(&id, result)).await;
            }
            Some("session/resume") => {
                locked(&view.0).resumed.push(msg["params"].clone());
                offered = Offered::from_params(&msg["params"]);
                put(&mut wire, &view, reply(&id, json!({}))).await;
            }
            Some(method @ ("session/set_config_option" | "session/set_model")) => {
                locked(&view.0)
                    .switches
                    .push((method.to_owned(), msg["params"].clone()));
                if offer == Offer::Stubborn {
                    let error = json!({"code": -32602, "message": "no"});
                    let line = json!({"jsonrpc": "2.0", "id": id, "error": error});
                    put(&mut wire, &view, line.to_string()).await;
                } else {
                    put(&mut wire, &view, reply(&id, json!({"configOptions": []}))).await;
                }
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
