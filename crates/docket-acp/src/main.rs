//! docket-acp: the ACP server edge as a process. An editor launches it and speaks ACP on its
//! stdin and stdout; logs go to stderr only. It refuses to serve unless the setting
//! `agent.acp.expose` is on. Exits 2 when that is off or the command line is bad, 1 for any other
//! stop.

use docket_acp::Permit;
use std::process::ExitCode;

const USAGE: &str =
    "usage: docket-acp [--app NAME]   (NAME: the editor's app name, default acp.editor)";

fn app(mut args: impl Iterator<Item = String>) -> Result<porter_core::AppName, String> {
    let name = match (args.next().as_deref(), args.next()) {
        (None, _) => "acp.editor".to_owned(),
        (Some("--app"), Some(name)) => name,
        _ => return Err(USAGE.to_owned()),
    };
    porter_core::AppName::parse(&name).map_err(|_| format!("not an app name: {name}"))
}

fn main() -> ExitCode {
    let editor = match app(std::env::args().skip(1)) {
        Ok(editor) => editor,
        Err(why) => {
            eprintln!("docket-acp: {why}");
            return ExitCode::from(2);
        }
    };
    if let Err(why) = Permit::from_env(&|k| std::env::var(k).ok()) {
        eprintln!("docket-acp: {why}");
        return ExitCode::from(2);
    }
    // The session host (the native backend over the router, S2) is not built yet: a served
    // connection would have nothing behind it. `docket_acp::Server` takes it when it exists.
    eprintln!("docket-acp: no session host is wired yet; nothing to serve for {editor}");
    ExitCode::FAILURE
}
