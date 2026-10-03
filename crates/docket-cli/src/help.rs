//! `--help`: the commands, the flags and the exit codes, which are part of the interface.

use crate::exit::Exit;

/// The help text.
pub fn text() -> String {
    let codes: Vec<String> = Exit::ALL
        .iter()
        .map(|e| {
            let meaning = match e {
                Exit::Done => "done",
                Exit::Usage => "usage: unknown app, action or parameter, or a bad value",
                Exit::Refused => "refused by policy",
                Exit::Declined => "the person declined, or the confirmation timed out",
                Exit::AppFailed => "the app failed the call",
                Exit::Unavailable => {
                    "unavailable: intentd is down, or the app is not installed or does not start"
                }
                Exit::Halted => "halted or paused (kill switch, breaker)",
            };
            format!("  {}  {meaning}", e.code())
        })
        .collect();
    include_str!("help.txt").replace("{codes}", &codes.join("\n"))
}
