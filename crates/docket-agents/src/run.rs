//! The package manager, as a seam: node packages are installed by `npm`, which checks them.

use std::path::Path;
use std::process::Command;

/// Why a program did not run to success.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0} did not finish")]
pub struct RunFault(pub String);

/// Runs a program to its end.
pub trait Run {
    /// Runs `program` with `args`, working in `dir`.
    fn run(&self, program: &str, args: &[String], dir: &Path) -> Result<(), RunFault>;
}

/// Runs the real program.
#[derive(Debug, Clone, Copy)]
pub struct System;

impl Run for System {
    fn run(&self, program: &str, args: &[String], dir: &Path) -> Result<(), RunFault> {
        let status = Command::new(program)
            .args(args)
            .current_dir(dir)
            .status()
            .map_err(|_| RunFault(program.to_owned()))?;
        status
            .success()
            .then_some(())
            .ok_or_else(|| RunFault(program.to_owned()))
    }
}
