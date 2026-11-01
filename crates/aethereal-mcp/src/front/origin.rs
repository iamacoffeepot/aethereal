//! Whose page a request may come from (ADR-0037).

/// This machine's names, as an origin writes them.
const LOCAL: [&str; 3] = ["localhost", "127.0.0.1", "[::1]"];

/// Whether `origin` is this machine's or one of `allowed`, each compared exactly.
pub fn is_allowed(origin: &str, allowed: &[String]) -> bool {
    let is_listed = allowed.iter().any(|listed| listed == origin);

    is_listed || is_local(origin)
}

/// Whether `origin` is `http` or `https` on one of this machine's names, on any port.
fn is_local(origin: &str) -> bool {
    let authority = origin.strip_prefix("http://").or_else(|| origin.strip_prefix("https://"));
    let Some(authority) = authority else {
        return false;
    };

    LOCAL.iter().any(|name| is_at(authority, name))
}

/// Whether `authority` is `name`, with a port or without.
fn is_at(authority: &str, name: &str) -> bool {
    let Some(rest) = authority.strip_prefix(name) else {
        return false;
    };
    let Some(port) = rest.strip_prefix(':') else {
        return rest.is_empty();
    };

    !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit())
}
