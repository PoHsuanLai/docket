//! Where bulkhead's types meet docket-core's: bulkhead owns its sandbox-state and network types
//! (it depends on no docket crate; the path is its own, shared), and these are the conversions at
//! the edge.

use bulkhead::{CannotSandbox as ShellCannot, Network, NetworkMode, SandboxState as ShellState};
use docket_core::{CannotSandbox, NetAccess, SandboxState};

/// Why bulkhead cannot confine, as docket-core words it.
pub const fn cannot(why: ShellCannot) -> CannotSandbox {
    match why {
        ShellCannot::NotInstalled => CannotSandbox::NotInstalled,
        ShellCannot::NamespacesDenied => CannotSandbox::NamespacesDenied,
        ShellCannot::RestrictedByAppArmor => CannotSandbox::RestrictedByAppArmor,
        ShellCannot::Unsupported => CannotSandbox::Unsupported,
        ShellCannot::BadWorkingDir => CannotSandbox::BadWorkingDir,
        ShellCannot::LimitsUnavailable => CannotSandbox::LimitsUnavailable,
        ShellCannot::SeccompUnavailable => CannotSandbox::SeccompUnavailable,
        ShellCannot::BadLimits => CannotSandbox::BadLimits,
        // A reason this build cannot name: the sandbox cannot run, so say unsupported.
        _ => CannotSandbox::Unsupported,
    }
}

/// bulkhead's sandbox state as docket-core's.
pub const fn state(state: ShellState) -> SandboxState {
    match state {
        ShellState::Ready => SandboxState::Ready,
        ShellState::Cannot(why) => SandboxState::Cannot(cannot(why)),
    }
}

/// The network a terminal's sandbox has.
pub const fn access(network: Network) -> NetAccess {
    match network {
        Network::Off => NetAccess::Closed,
        Network::Host => NetAccess::Open,
        // Unknown network: assume it reaches out, so every command is treated as net-capable.
        _ => NetAccess::Open,
    }
}

/// The network an agent process's sandbox has: anything but none can reach something.
pub const fn mode_access(mode: NetworkMode) -> NetAccess {
    match mode {
        NetworkMode::None => NetAccess::Closed,
        // An allowlist reaches the named hosts: open, as far as a command is concerned.
        NetworkMode::EndpointOnly | NetworkMode::Allowlist | NetworkMode::Host => NetAccess::Open,
        // Unknown mode: assume it reaches out, so every command is treated as net-capable.
        _ => NetAccess::Open,
    }
}
