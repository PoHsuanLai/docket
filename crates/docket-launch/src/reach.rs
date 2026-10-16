//! The sites an agent on the `allowlist` network may reach: the lists written in `agents.toml`
//! (`allow`, `deny`, `ports`), checked when the file is read. A site that is not a host or a
//! pattern refuses the whole file and names the program and the entry; an allow list with
//! nothing in it refuses too, because it would reach nothing and is surely a mistake.

use crate::config::ConfigFault;
use bulkhead::egress::{DEFAULT_PORTS, EgressPolicy, Host, HostPattern, Ports};

/// The checked lists of one program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reach {
    policy: EgressPolicy,
    sites: Vec<String>,
}

/// One entry of a list, parsed, or the fault that names it.
fn pattern(program: &str, list: &'static str, entry: &str) -> Result<HostPattern, ConfigFault> {
    HostPattern::parse(entry).map_err(|why| ConfigFault::Site {
        program: program.to_owned(),
        list,
        entry: entry.to_owned(),
        why,
    })
}

fn patterns(
    program: &str,
    list: &'static str,
    entries: &[String],
) -> Result<Vec<HostPattern>, ConfigFault> {
    entries.iter().map(|e| pattern(program, list, e)).collect()
}

/// An allowed entry as a person reads it.
fn shown(pattern: &HostPattern) -> String {
    match pattern {
        HostPattern::Exact(name) => name.clone(),
        HostPattern::Subdomains(domain) => format!("*.{domain}"),
        HostPattern::Addr(addr) => Host::Addr(*addr).to_string(),
    }
}

fn fault(program: &str, why: &'static str) -> ConfigFault {
    ConfigFault::Entry {
        program: program.to_owned(),
        why,
    }
}

impl Reach {
    /// The lists of `program`. `extra_ports` are reachable besides 443 and 80.
    pub fn new(
        program: &str,
        allow: &[String],
        deny: &[String],
        extra_ports: &[u16],
    ) -> Result<Self, ConfigFault> {
        let allowed = patterns(program, "allow", allow)?;
        let denied = patterns(program, "deny", deny)?;
        let sites = allowed.iter().map(shown).collect();
        let ports = Ports::new(DEFAULT_PORTS.iter().chain(extra_ports).copied().collect())
            .map_err(|_| fault(program, "a port is 1 to 65535"))?;
        let policy = EgressPolicy::new(allowed, denied, ports).map_err(|_| {
            fault(
                program,
                "network allowlist needs at least one entry in allow",
            )
        })?;
        Ok(Self { policy, sites })
    }

    /// The policy the filter enforces.
    pub fn policy(&self) -> &EgressPolicy {
        &self.policy
    }

    /// The allowed sites, as written (lower-cased), for showing the person.
    pub fn sites(&self) -> &[String] {
        &self.sites
    }
}
