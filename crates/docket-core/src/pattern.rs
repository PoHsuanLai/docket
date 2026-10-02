//! Matching a value against the trusted patterns of a task policy. A pattern traces to the
//! person's own words or choice; a value matches by whole labels and whole path segments,
//! never by substring, so `evil-example.com` is not inside `example.com`.

use crate::ids::FileRef;
use crate::task_policy::TrustedPattern;
use crate::value::Value;

/// Whether `value` is inside `pattern`.
pub(crate) fn pattern_matches(pattern: &TrustedPattern, value: &Value) -> bool {
    match (pattern, value) {
        (TrustedPattern::Exact(want), got) => want == got,
        (TrustedPattern::Entity(want), Value::Entity(got)) => want == got,
        (TrustedPattern::Entity(want), Value::Entities(got)) => {
            !got.is_empty() && got.iter().all(|e| e == want)
        }
        (TrustedPattern::Domain(domain), Value::Text(text)) => text
            .rsplit_once('@')
            .is_some_and(|(_, host)| in_domain(host, domain)),
        (TrustedPattern::Domain(domain), Value::Url(url)) => {
            host_of_url(url).is_some_and(|host| in_domain(host, domain))
        }
        (TrustedPattern::Under(base), Value::File(file)) => under(file, base),
        (TrustedPattern::Entity(_) | TrustedPattern::Domain(_) | TrustedPattern::Under(_), _) => {
            false
        }
    }
}

/// Whether everything `inner` matches is also matched by `outer`.
pub(crate) fn pattern_inside(inner: &TrustedPattern, outer: &TrustedPattern) -> bool {
    match (inner, outer) {
        (TrustedPattern::Exact(value), outer) => pattern_matches(outer, value),
        (TrustedPattern::Domain(a), TrustedPattern::Domain(b)) => in_domain(a, b),
        (TrustedPattern::Under(a), TrustedPattern::Under(b)) => under(a, b),
        (TrustedPattern::Entity(a), TrustedPattern::Entity(b)) => a == b,
        (TrustedPattern::Domain(_) | TrustedPattern::Under(_) | TrustedPattern::Entity(_), _) => {
            false
        }
    }
}

fn in_domain(host: &str, domain: &str) -> bool {
    let (host, domain) = (host.to_ascii_lowercase(), domain.to_ascii_lowercase());
    !domain.is_empty()
        && (host == domain
            || host
                .strip_suffix(&domain)
                .is_some_and(|rest| rest.ends_with('.')))
}

/// The host of `scheme://user@host:port/path`.
fn host_of_url(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?;
    let host = host.split(':').next()?;
    (!host.is_empty()).then_some(host)
}

fn under(file: &FileRef, base: &FileRef) -> bool {
    let (path, base) = (file.as_str(), base.as_str().trim_end_matches('/'));
    let climbs = path.split('/').any(|segment| segment == "..");
    !climbs
        && (path == base
            || path
                .strip_prefix(base)
                .is_some_and(|rest| rest.starts_with('/')))
}
