//! Where bulkhead's types meet docket-core's: bulkhead owns its own path, sandbox-state and
//! network types (it depends on no docket crate), and these are the conversions at the edge.

use bulkhead::{CannotSandbox as ShellCannot, Network, NetworkMode, SandboxState as ShellState};
use docket_core::{AbsPath, CannotSandbox, NetAccess, SandboxState};

/// A docket path as bulkhead's. Both hold absolute normalised text, so this fails only if one of
/// the two ever disagrees with the other about what that means.
pub fn to_shell(path: &AbsPath) -> Result<bulkhead::AbsPath, bulkhead::PathFault> {
    bulkhead::AbsPath::parse(path.as_str())
}

/// A bulkhead path as docket's.
pub fn from_shell(path: &bulkhead::AbsPath) -> Result<AbsPath, docket_core::PathFault> {
    AbsPath::parse(path.as_str())
}

/// Why bulkhead cannot confine, as docket-core words it.
pub const fn cannot(why: ShellCannot) -> CannotSandbox {
    match why {
        ShellCannot::NotInstalled => CannotSandbox::NotInstalled,
        ShellCannot::NamespacesDenied => CannotSandbox::NamespacesDenied,
        ShellCannot::Unsupported => CannotSandbox::Unsupported,
        ShellCannot::BadWorkingDir => CannotSandbox::BadWorkingDir,
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
    }
}

/// The network an agent process's sandbox has: anything but none can reach something.
pub const fn mode_access(mode: NetworkMode) -> NetAccess {
    match mode {
        NetworkMode::None => NetAccess::Closed,
        NetworkMode::EndpointOnly | NetworkMode::Host => NetAccess::Open,
    }
}
