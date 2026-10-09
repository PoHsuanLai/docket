//! Python packages from the python package index, installed by `uv` into the agent's own
//! directory: a virtual environment under `files`, the cache there too. The package manager
//! checks its own packages; docket only names them and asks for nothing but the index.

use crate::install::InstallFault;
use crate::run::Run;
use crate::slug::Slug;
use crate::snapshot::Package;
use std::path::Path;

/// A package the registry names, reduced to what `uv pip install` is given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonPackage {
    /// The requirement as uv takes it (`name` or `name==1.2.3`).
    pub requirement: String,
    /// The distribution name, which is also the program it installs.
    pub name: String,
}

/// Why a package string is not a name with an optional version.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a package name with an optional version")]
pub struct SpecRefused(String);

impl PythonPackage {
    /// Reads `name`, `name==1.2.3` or `name@1.2.3`, the forms the registry uses. URLs, direct
    /// references and anything with spaces are refused: only the package index is asked.
    pub fn parse(text: &str) -> Result<Self, SpecRefused> {
        let refused = || SpecRefused(text.to_owned());
        if text.contains("://") || text.contains(char::is_whitespace) || text.contains(';') {
            return Err(refused());
        }
        let requirement = match text.split_once('@') {
            Some((name, version)) => format!("{name}=={version}"),
            None => text.to_owned(),
        };
        let name = requirement
            .split(['[', '=', '<', '>', '!', '~'])
            .next()
            .unwrap_or_default();
        Slug::parse(name).map_err(|_| refused())?;
        Ok(Self {
            name: name.to_owned(),
            requirement,
        })
    }
}

fn words(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| (*p).to_owned()).collect()
}

/// Installs a python package under `files` and returns the path of its program, relative. The
/// environment is made inside `files` (relative paths, so nothing is written outside it), and
/// `--no-python-downloads` keeps uv from fetching an interpreter into the person's home.
pub fn uvx(package: &Package, files: &Path, run: &dyn Run) -> Result<String, InstallFault> {
    let python = PythonPackage::parse(&package.package)
        .map_err(|_| InstallFault::Unsupported(package.package.clone()))?;
    run.run(
        "uv",
        &words(&[
            "--cache-dir",
            "cache",
            "--no-python-downloads",
            "venv",
            "venv",
        ]),
        files,
    )?;
    run.run(
        "uv",
        &words(&[
            "--cache-dir",
            "cache",
            "pip",
            "install",
            "--python",
            "venv/bin/python",
            &python.requirement,
        ]),
        files,
    )?;
    files
        .join("venv/bin")
        .join(&python.name)
        .is_file()
        .then_some(())
        .ok_or(InstallFault::NoProgram)?;
    Ok(format!("venv/bin/{}", python.name))
}
