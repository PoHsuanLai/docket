//! Where an external agent's file calls may reach: the session's working directory and nothing
//! else. Pure: the lexical rules here, and one fact from the file system (the path after
//! symlinks) passed in. A path the agent names is refused if it is not absolute, steps up with
//! `..`, lies outside the directory, resolves outside it through a link, or names a place that
//! holds the person's secrets (even inside the directory).
//!
//! A write to a place that can later run code outside the sandbox (`.git`, an editor or agent
//! configuration) is `Sensitive`: it asks every time and is never offered "always".

use docket_core::{AbsPath, Cover};

/// Directory names that hold secrets wherever they sit.
const SECRET_DIRS: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".aws",
    ".kube",
    ".password-store",
    ".mozilla",
];
/// Two-step paths that hold secrets.
const SECRET_PAIRS: &[(&str, &str)] = &[
    (".config", "accountd"),
    (".config", "gcloud"),
    (".config", "chromium"),
    (".config", "google-chrome"),
    (".local", "share/keyrings"),
];
/// File names that are credentials.
const SECRET_FILES: &[&str] = &[
    ".netrc",
    ".git-credentials",
    ".pgpass",
    "id_rsa",
    "id_ecdsa",
    "id_ed25519",
];
/// Directory and file names whose contents later run with the person's rights.
const SENSITIVE: &[&str] = &[
    ".git",
    ".claude",
    ".vscode",
    ".idea",
    ".husky",
    ".envrc",
    ".bashrc",
    ".zshrc",
    ".profile",
    ".gitmodules",
];

/// Why a path was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// Not an absolute path, or it steps up.
    #[error("not an absolute path without ..")]
    BadPath,
    /// Outside the session's directory.
    #[error("outside the session's directory")]
    Outside,
    /// Inside by name but a link leads out.
    #[error("a link leads outside the session's directory")]
    Link,
    /// A place that holds the person's secrets.
    #[error("a place that holds secrets")]
    Secret,
}

/// How much care a path needs for a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Care {
    /// An ordinary file.
    Plain,
    /// Something that later runs code: asks every time.
    Sensitive,
}

/// A path that passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confined {
    /// The path as requested (normalised).
    pub path: AbsPath,
    /// The path after links: what is really touched.
    pub real: AbsPath,
    /// How much care a write needs.
    pub care: Care,
}

fn parts(path: &AbsPath) -> Vec<&str> {
    path.as_str().split('/').filter(|p| !p.is_empty()).collect()
}

fn secret(path: &AbsPath) -> bool {
    let p = parts(path);
    let pair = |a: &str, b: &str| {
        SECRET_PAIRS
            .iter()
            .any(|(first, rest)| *first == a && *rest == b)
    };
    p.iter()
        .any(|c| SECRET_DIRS.contains(c) || SECRET_FILES.contains(c))
        || p.windows(2).any(|w| pair(w[0], w[1]))
        || p.windows(3)
            .any(|w| pair(w[0], &format!("{}/{}", w[1], w[2])))
}

fn care(path: &AbsPath) -> Care {
    if parts(path).iter().any(|c| SENSITIVE.contains(c)) {
        Care::Sensitive
    } else {
        Care::Plain
    }
}

/// The lexical half: parse and check the name against `scope`. The caller then resolves the
/// path's links and passes the result to [`confine`].
pub fn named(scope: &AbsPath, requested: &str) -> Result<AbsPath, Refusal> {
    let path = AbsPath::parse(requested).map_err(|_| Refusal::BadPath)?;
    if scope.covers(&path) != Cover::Covers {
        return Err(Refusal::Outside);
    }
    Ok(path)
}

/// The whole rule, given the links resolved: `real_scope` is the scope after links, `real` the
/// path after links.
pub fn confine(real_scope: &AbsPath, path: AbsPath, real: AbsPath) -> Result<Confined, Refusal> {
    if real_scope.covers(&real) != Cover::Covers {
        return Err(Refusal::Link);
    }
    if secret(&path) || secret(&real) {
        return Err(Refusal::Secret);
    }
    let care = if care(&path) == Care::Sensitive || care(&real) == Care::Sensitive {
        Care::Sensitive
    } else {
        Care::Plain
    };
    Ok(Confined { path, real, care })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abs(t: &str) -> AbsPath {
        AbsPath::parse(t).expect("abs")
    }

    #[test]
    fn names_are_checked_before_the_file_system_is_asked() {
        let scope = abs("/work/app");
        assert!(named(&scope, "/work/app/src/a.rs").is_ok());
        assert_eq!(named(&scope, "src/a.rs"), Err(Refusal::BadPath));
        assert_eq!(
            named(&scope, "/work/app/../etc/passwd"),
            Err(Refusal::BadPath)
        );
        assert_eq!(
            named(&scope, "/work/app/src/../../x"),
            Err(Refusal::BadPath)
        );
        assert_eq!(named(&scope, "/work/application/a"), Err(Refusal::Outside));
        assert_eq!(named(&scope, "/etc/passwd"), Err(Refusal::Outside));
        assert_eq!(named(&scope, "/work/app/a\nb"), Err(Refusal::BadPath));
    }

    #[test]
    fn a_link_that_leads_out_is_refused() {
        let scope = abs("/work/app");
        let path = abs("/work/app/link/x");
        assert_eq!(
            confine(&scope, path.clone(), abs("/etc/x")),
            Err(Refusal::Link)
        );
        assert!(confine(&scope, path.clone(), abs("/work/app/real/x")).is_ok());
    }

    #[test]
    fn secrets_are_refused_even_inside_the_directory() {
        let scope = abs("/home/u/proj");
        for p in [
            "/home/u/proj/.ssh/id_ed25519",
            "/home/u/proj/sub/.aws/credentials",
            "/home/u/proj/.netrc",
            "/home/u/proj/.config/accountd/keys",
            "/home/u/proj/.local/share/keyrings/login.keyring",
        ] {
            assert_eq!(confine(&scope, abs(p), abs(p)), Err(Refusal::Secret), "{p}");
        }
        let fine = abs("/home/u/proj/.config/app/x");
        assert!(confine(&scope, fine.clone(), fine).is_ok());
    }

    #[test]
    fn code_that_runs_later_is_sensitive() {
        let scope = abs("/work/app");
        for (p, want) in [
            ("/work/app/.git/hooks/pre-commit", Care::Sensitive),
            ("/work/app/.claude/settings.json", Care::Sensitive),
            ("/work/app/.vscode/tasks.json", Care::Sensitive),
            ("/work/app/src/main.rs", Care::Plain),
        ] {
            assert_eq!(confine(&scope, abs(p), abs(p)).expect(p).care, want, "{p}");
        }
    }
}
