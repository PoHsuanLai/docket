//! Reading one `[[agent]]` table into an [`Entry`]: every field checked, and the route that
//! decides what the entry must say.

use super::{ConfigFault, Entry, Profile, Raw, Route, SESSION_MARK};
use crate::reach::Reach;
use agents_core::dirs::AgentsDir;
use bulkhead::NetworkMode;
use docket_acp_host::{ModelId, SignIn};
use docket_core::ProgramName;
use docket_core::{AbsPath, Rewind};

use porter_core::DataClass;
use porter_core::capability::{AgentProgram, AgentProtocol, EnvName};

/// The longest label, in characters.
const LABEL_MAX: usize = 64;

fn label(program: &str, text: Option<String>) -> Result<Option<prov::AgentLabel>, ConfigFault> {
    let Some(text) = text else { return Ok(None) };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(bad(program, "a label may not be empty"));
    }
    if trimmed.chars().count() > LABEL_MAX {
        return Err(bad(program, "a label is at most 64 characters"));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(bad(
            program,
            "a label holds no control character or newline",
        ));
    }
    Ok(Some(prov::AgentLabel(trimmed.to_owned())))
}

fn bad(program: &str, why: &'static str) -> ConfigFault {
    ConfigFault::Entry {
        program: program.to_owned(),
        why,
    }
}

/// The arguments that pick a conversation up, as written, or the well-known flag of the
/// `claude-code` profile. One argument at least names the conversation, and none is empty.
fn resume_of(
    at: &str,
    written: Option<Vec<String>>,
    profile: Option<Profile>,
) -> Result<Option<Vec<String>>, ConfigFault> {
    let Some(args) = written else {
        return Ok(
            profile.map(|Profile::ClaudeCode| vec!["--resume".to_owned(), SESSION_MARK.to_owned()])
        );
    };
    let names_it = args.iter().any(|arg| arg.contains(SESSION_MARK));
    let fine = names_it
        && args
            .iter()
            .all(|arg| !arg.is_empty() && !arg.contains('\0'));
    match fine {
        true => Ok(Some(args)),
        false => Err(bad(
            at,
            "resume needs arguments, one of them naming {session}",
        )),
    }
}

fn abs(program: &str, text: &str) -> Result<AbsPath, ConfigFault> {
    let path = AbsPath::parse(text).map_err(|_| bad(program, "a path must be absolute"))?;
    match path.is_root() {
        docket_core::RootState::Root => Err(bad(program, "a path must not be /")),
        docket_core::RootState::Below => Ok(path),
    }
}

fn env(program: &str, name: Option<&String>) -> Result<Option<EnvName>, ConfigFault> {
    name.map(|n| EnvName::parse(n).map_err(|_| bad(program, "not an environment variable name")))
        .transpose()
}

/// Variables the person's `set` table may not touch: the dynamic linker, the search path, and
/// anything that runs code before the program does.
fn forbidden(name: &str) -> bool {
    name.starts_with("LD_")
        || matches!(
            name,
            "PATH" | "HOME" | "BASH_ENV" | "ENV" | "IFS" | "PYTHONSTARTUP"
        )
        || name.starts_with("DOCKET_")
}

/// What starts the program and what it may see, from `command` or from the registry.
struct Source {
    command: AbsPath,
    args: Vec<String>,
    reads: Vec<AbsPath>,
    state: Vec<AbsPath>,
    home: Option<AbsPath>,
    set: Vec<(String, String)>,
}

fn source(at: &str, raw: &Raw, dir: Option<&AgentsDir>) -> Result<Source, ConfigFault> {
    let paths = |list: &[String]| -> Result<Vec<AbsPath>, ConfigFault> {
        list.iter().map(|p| abs(at, p)).collect()
    };
    let args = raw.args.clone();
    let set = raw.set.clone().into_iter().collect::<Vec<_>>();
    match (&raw.command, &raw.registry, &raw.version) {
        (Some(command), None, None) => Ok(Source {
            command: abs(at, command)?,
            args,
            reads: paths(&raw.reads)?,
            state: paths(&raw.state)?,
            home: raw.home.as_deref().map(|p| abs(at, p)).transpose()?,
            set,
        }),
        (None, Some(_), Some(_)) if !raw.state.is_empty() || raw.home.is_some() => Err(bad(
            at,
            "a registry agent has a home of its own: no state or home",
        )),
        (None, Some(id), Some(version)) => {
            let dir = dir.ok_or_else(|| bad(at, "a registry agent needs the agents directory"))?;
            let launch =
                crate::registry::launch(dir, id, version).map_err(|why| ConfigFault::Registry {
                    program: at.to_owned(),
                    why,
                })?;
            let path = |p: &std::path::Path| {
                p.to_str()
                    .ok_or_else(|| bad(at, "the agents directory is not UTF-8"))
                    .and_then(|t| abs(at, t))
            };
            let home = path(&launch.home)?;
            let reads = [path(&launch.files)?]
                .into_iter()
                .chain(paths(&raw.reads)?)
                .collect();
            Ok(Source {
                command: path(&launch.command)?,
                args: launch.args.into_iter().chain(args).collect(),
                reads,
                state: vec![home.clone()],
                home: Some(home),
                set: launch.env.into_iter().chain(set).collect(),
            })
        }
        (None, Some(_), None) => Err(bad(at, "a registry agent needs a pinned version")),
        _ => Err(bad(
            at,
            "name either command, or registry with version (not both)",
        )),
    }
}

pub(super) fn check(raw: Raw, dir: Option<&AgentsDir>) -> Result<Entry, ConfigFault> {
    let at = raw.program.clone();
    let name = ProgramName::parse(&raw.program).map_err(|_| bad(&at, "not a program name"))?;
    let program = AgentProgram::parse(&raw.program).map_err(|_| {
        bad(
            &at,
            "a program is lower-case letters, digits and - (porter's grammar)",
        )
    })?;
    let src = source(&at, &raw, dir)?;
    let route = raw
        .route
        .or_else(|| raw.registry.as_ref().map(|_| Route::Login))
        .ok_or_else(|| bad(&at, "route is required"))?;
    if src.args.iter().any(|a| a.contains('\0')) {
        return Err(bad(&at, "an argument holds a NUL"));
    }
    let label = label(&at, raw.label.clone())?;
    let key_env = env(&at, raw.key_env.as_ref())?;
    let base_url_env = env(&at, raw.base_url_env.as_ref())?;
    let set = src
        .set
        .iter()
        .map(|(k, v)| {
            let name =
                EnvName::parse(k).map_err(|_| bad(&at, "not an environment variable name"))?;
            let fine = !forbidden(k)
                && Some(&name) != key_env.as_ref()
                && Some(&name) != base_url_env.as_ref()
                && !v.chars().any(char::is_control);
            if fine {
                Ok((name, v.clone()))
            } else {
                Err(bad(
                    &at,
                    "a `set` variable may not be a key, a base URL, PATH, HOME or a loader switch",
                ))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    // The route decides what the entry must say, and what network makes sense.
    match (route, raw.network) {
        (Route::Endpoint, NetworkMode::None | NetworkMode::Allowlist) => {
            return Err(bad(
                &at,
                "the endpoint route needs endpoint_only (or host) network",
            ));
        }
        (Route::Handoff, NetworkMode::None | NetworkMode::EndpointOnly) => {
            return Err(bad(
                &at,
                "a handed-off key is for a program that reaches its provider: network allowlist or host",
            ));
        }
        (_, NetworkMode::EndpointOnly) if route != Route::Endpoint => {
            return Err(bad(&at, "endpoint_only is for the endpoint route"));
        }
        (
            _,
            NetworkMode::None
            | NetworkMode::EndpointOnly
            | NetworkMode::Allowlist
            | NetworkMode::Host,
        ) => {}
        _ => return Err(bad(&at, "a network this version does not know")),
    }
    let reach = match raw.network {
        NetworkMode::Allowlist => Some(Reach::new(&at, &raw.allow, &raw.deny, &raw.ports)?),
        _ if raw.allow.is_empty() && raw.deny.is_empty() && raw.ports.is_empty() => None,
        _ => return Err(bad(&at, "allow, deny and ports go with network allowlist")),
    };
    if route == Route::Endpoint && (raw.base_url_env.is_none() || raw.key_env.is_none()) {
        return Err(bad(
            &at,
            "the endpoint route needs base_url_env and key_env",
        ));
    }
    if route == Route::Endpoint && raw.endpoint.is_none() {
        return Err(bad(
            &at,
            "the endpoint route needs an [agent.endpoint] table",
        ));
    }
    if route == Route::Handoff && raw.key_env.is_none() {
        return Err(bad(&at, "the handoff route needs key_env"));
    }
    Ok(Entry {
        name,
        program,
        command: src.command,
        args: src.args,
        route,
        network: raw.network,
        reach,
        key_env,
        base_url_env,
        protocol: raw.protocol.unwrap_or(AgentProtocol::AnthropicMessages),
        class: raw.class.unwrap_or(DataClass::Files),
        delivery: raw.delivery,
        endpoint: raw.endpoint,
        reads: src.reads,
        state: src.state,
        home: src.home,
        set,
        login: raw.login,
        logout: raw.logout,
        tools: raw.tools,
        label,
        profile: raw.profile,
        sign_in: raw
            .sign_in
            .as_deref()
            .map(SignIn::parse)
            .transpose()
            .map_err(|_| bad(&at, "sign_in is 1 to 64 letters, digits, - _ or ."))?,
        model: raw
            .model
            .as_deref()
            .map(ModelId::parse)
            .transpose()
            .map_err(|_| bad(&at, "a model id has no spaces or control characters"))?,
        registry: raw.registry.clone(),
        rewind: raw.checkpoints.unwrap_or(match raw.profile {
            Some(Profile::ClaudeCode) => Rewind::Agent,
            None => Rewind::Docket,
        }),
        resume: resume_of(&at, raw.resume, raw.profile)?,
    })
}
