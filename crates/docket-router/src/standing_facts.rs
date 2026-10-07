//! What a call's typed arguments say about where it writes, runs or sends: the facts a standing
//! grant is matched against. Read off the manifest's sinks, never off words an agent supplied
//! beyond the typed values; anything unreadable is `Opaque`, which no grant covers.

use docket_core::{
    AbsPath, ActionDecl, ArgFacts, ArgSink, CallFacts, CallRequest, Domain, ParamName, Recipient,
    TargetValue, Value,
};

/// The parameters a terminal action names its command and working directory by.
pub(crate) const COMMAND_PARAM: &str = "command";
/// See [`COMMAND_PARAM`].
pub(crate) const CWD_PARAM: &str = "cwd";

fn text_of(value: &Value) -> Option<&str> {
    match value {
        Value::Text(t) | Value::Url(t) => Some(t),
        Value::File(f) => Some(f.as_str()),
        Value::Entity(e) => Some(e.key.as_str()),
        _ => None,
    }
}

/// Every leaf of a value that is one thing (a list is its items).
fn leaves(value: &Value) -> Vec<&Value> {
    match value {
        Value::List(items) => items.iter().flat_map(leaves).collect(),
        Value::Entities(_) | Value::Record(_) | Value::Handle(_) => vec![value],
        one => vec![one],
    }
}

/// The host a URL names, none when it hides one behind user information or has none.
fn host_of(url: &str) -> Option<Domain> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.contains('@') {
        return None;
    }
    let host = authority.split(':').next()?;
    Domain::parse(host).ok()
}

fn recipient_of(value: &Value) -> Option<Recipient> {
    match value {
        Value::Url(u) => host_of(u).map(Recipient::Domain),
        other => text_of(other).and_then(|t| Recipient::address(t).ok()),
    }
}

fn path_of(value: &Value) -> Option<AbsPath> {
    match value {
        Value::File(_) | Value::Text(_) => text_of(value).and_then(|t| AbsPath::parse(t).ok()),
        _ => None,
    }
}

/// Collects `parse` of every leaf of the arguments feeding `sink`; `None` when a leaf could not
/// be read.
fn gather<T>(
    decl: &ActionDecl,
    request: &CallRequest,
    sink: ArgSink,
    parse: fn(&Value) -> Option<T>,
) -> Option<Vec<T>> {
    let mut out = Vec::new();
    for param in decl.params.iter().filter(|p| p.sink == sink) {
        if let Some(arg) = request.args.get(&param.name) {
            for leaf in leaves(&arg.value) {
                out.push(parse(leaf)?);
            }
        }
    }
    Some(out)
}

fn named<'a>(request: &'a CallRequest, name: &str) -> Option<&'a Value> {
    ParamName::parse(name)
        .ok()
        .and_then(|n| request.args.get(&n))
        .map(|a| &a.value)
}

/// The facts of a call: a command and its directory when the action names them, else the paths
/// it writes or removes, else the recipients it sends to, else nothing a scope can name.
pub(crate) fn facts_of(decl: &ActionDecl, request: &CallRequest) -> CallFacts {
    let action = request.action.clone();
    let args = args_of(decl, request);
    CallFacts { action, args }
}

fn args_of(decl: &ActionDecl, request: &CallRequest) -> ArgFacts {
    if let Some(command) = named(request, COMMAND_PARAM) {
        let line = match command {
            Value::Text(t) => t.clone(),
            _ => return ArgFacts::Opaque,
        };
        return match named(request, CWD_PARAM).and_then(path_of) {
            Some(cwd) => ArgFacts::Command { line, cwd },
            None => ArgFacts::Opaque,
        };
    }
    let mut paths = match gather(decl, request, ArgSink::Path, path_of) {
        Some(paths) => paths,
        None => return ArgFacts::Opaque,
    };
    if let TargetValue::Files(files) = &request.target {
        for file in files {
            match AbsPath::parse(file.as_str()) {
                Ok(path) => paths.push(path),
                Err(_) => return ArgFacts::Opaque,
            }
        }
    }
    let mut to = match gather(decl, request, ArgSink::Recipient, recipient_of) {
        Some(to) => to,
        None => return ArgFacts::Opaque,
    };
    match gather(decl, request, ArgSink::Destination, recipient_of) {
        Some(more) => to.extend(more),
        None => return ArgFacts::Opaque,
    }
    match (paths.is_empty(), to.is_empty()) {
        (false, true) => ArgFacts::Paths(paths),
        (true, false) => ArgFacts::Recipients(to),
        (true, true) => ArgFacts::Unscoped,
        (false, false) => ArgFacts::Opaque,
    }
}
