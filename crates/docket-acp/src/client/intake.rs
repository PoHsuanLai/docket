//! A line from the agent, sorted. Requests are read into the schema crate's types here, so a
//! request whose parameters do not fit is `Bad` before anything looks at it; a method we do not
//! serve is `Unknown`, answered "method not found" and never silently accepted.

use crate::wire::Incoming;
use agent_client_protocol_schema::rpc::RequestId;
use agent_client_protocol_schema::v1::{
    CLIENT_METHOD_NAMES, CreateTerminalRequest, ReadTextFileRequest, RequestPermissionRequest,
    SessionNotification, WriteTextFileRequest,
};
use serde_json::Value;

/// What an agent's request asks for.
#[derive(Debug, Clone)]
pub enum Work {
    /// `fs/read_text_file`.
    Read(ReadTextFileRequest),
    /// `fs/write_text_file`.
    Write(WriteTextFileRequest),
    /// `session/request_permission`.
    Permission(Box<RequestPermissionRequest>),
    /// `terminal/create`: a command, announced as a call.
    Create {
        /// The raw parameters.
        params: Value,
        /// The parsed request (for its session and its command).
        request: Box<CreateTerminalRequest>,
    },
    /// `terminal/output`, `wait_for_exit`, `kill` or `release`.
    Terminal {
        /// The method.
        method: String,
        /// The parameters.
        params: Value,
    },
    /// `elicitation/create`: declined, we offer none.
    Elicitation,
    /// Not a method we serve.
    Unknown,
    /// A method we serve with parameters that do not fit it.
    Bad,
}

/// One line, sorted.
#[derive(Debug, Clone)]
pub enum Intake {
    /// The answer to something we asked.
    Reply {
        /// What we asked.
        id: RequestId,
        /// The result or the error object.
        outcome: Result<Value, Value>,
    },
    /// A `session/update`.
    Update(Box<SessionNotification>),
    /// A request that wants an answer.
    Request {
        /// To answer with.
        id: RequestId,
        /// What it wants.
        work: Work,
    },
    /// A notification we ignore, or a line that is not JSON-RPC (a stray log line).
    Ignore,
}

fn typed<T: serde::de::DeserializeOwned>(params: Value) -> Option<T> {
    serde_json::from_value(params).ok()
}

fn work(method: &str, params: Value) -> Work {
    let names = CLIENT_METHOD_NAMES;
    match method {
        m if m == names.fs_read_text_file => typed(params).map_or(Work::Bad, Work::Read),
        m if m == names.fs_write_text_file => typed(params).map_or(Work::Bad, Work::Write),
        m if m == names.session_request_permission => typed(params)
            .map_or(Work::Bad, |r: RequestPermissionRequest| {
                Work::Permission(Box::new(r))
            }),
        m if m == names.terminal_create => match typed::<CreateTerminalRequest>(params.clone()) {
            Some(request) => Work::Create {
                params,
                request: Box::new(request),
            },
            None => Work::Bad,
        },
        m if [
            names.terminal_output,
            names.terminal_wait_for_exit,
            names.terminal_kill,
            names.terminal_release,
        ]
        .contains(&m) =>
        {
            Work::Terminal {
                method: method.to_owned(),
                params,
            }
        }
        m if m == names.elicitation_create => Work::Elicitation,
        _ => Work::Unknown,
    }
}

/// Sorts `line`.
pub fn intake(line: &str) -> Intake {
    match Incoming::parse(line) {
        Err(_) => Intake::Ignore,
        Ok(Incoming::Reply { id, outcome }) => Intake::Reply { id, outcome },
        Ok(Incoming::Request { id, method, params }) => Intake::Request {
            id,
            work: work(&method, params),
        },
        Ok(Incoming::Notification { method, params }) => {
            if method == CLIENT_METHOD_NAMES.session_update {
                typed(params).map_or(Intake::Ignore, |n| Intake::Update(Box::new(n)))
            } else {
                Intake::Ignore
            }
        }
    }
}
