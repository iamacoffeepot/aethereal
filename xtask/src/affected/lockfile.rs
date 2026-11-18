//! What the lockfile records that the package graph does not: each registry package's checksum.
//!
//! `cargo metadata` names a package by its name, version, and source, and leaves its checksum out, so two graphs can
//! agree on a package whose content differs. A registry never serves two contents under one version, so this is a
//! change that should not happen; when it does, nothing says which members built the old content, and every package
//! is selected.

use crate::failure::Failure;
use std::collections::BTreeMap;
use toml::{Table, Value, de};

/// The first package both lockfiles name by the same name, version, and source, under a different checksum, as
/// `name@version`, where one is.
///
/// A lockfile that does not parse fails: what its change reaches cannot be told.
pub fn rechecksummed(base: &str, head: &str) -> Result<Option<String>, Failure> {
    let base = checksums(base).map_err(|error| Failure::new(format_args!("failed base-lockfile: {error}")))?;
    let head = checksums(head).map_err(|error| Failure::new(format_args!("failed head-lockfile: {error}")))?;

    let changed = head.iter().find(|(identity, checksum)| base.get(*identity).is_some_and(|old| old != *checksum));
    Ok(changed.map(|(identity, _)| format!("{}@{}", identity.name, identity.version)))
}

/// What names a package in a lockfile.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Identity {
    name: String,
    version: String,
    source: String,
}

/// Each `[[package]]` with a checksum, by what names it: a registry's packages. A workspace or path package has none.
fn checksums(lockfile: &str) -> Result<BTreeMap<Identity, String>, de::Error> {
    let lockfile: Table = lockfile.parse()?;
    let packages = match lockfile.get("package") {
        Some(Value::Array(packages)) => packages.as_slice(),
        _ => &[],
    };

    let checksummed = packages.iter().filter_map(Value::as_table).filter_map(|package| {
        let field = |key: &str| package.get(key).and_then(Value::as_str).map(str::to_owned);
        let identity = Identity { name: field("name")?, version: field("version")?, source: field("source")? };
        Some((identity, field("checksum")?))
    });
    Ok(checksummed.collect())
}

#[cfg(test)]
mod tests {
    use super::rechecksummed;

    /// A lockfile holding `serde` 1.0.0 from crates.io under `checksum`, and the workspace's own `a`.
    fn lockfile(checksum: &str) -> String {
        format!(
            r#"version = 4

[[package]]
name = "a"
version = "0.1.0"
dependencies = ["serde"]

[[package]]
name = "serde"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "{checksum}"
"#
        )
    }

    #[test]
    fn a_checksum_changed_under_one_version_is_found_and_a_version_bump_is_not() {
        // Catches content changed under an unchanged identity passed as no change, which the graph diff cannot see,
        // and a bump, which changes the version alongside the checksum, sent to every package when the graph diff
        // finds what it reaches.
        let base = lockfile("aa");
        let bumped = lockfile("bb").replace(r#"version = "1.0.0""#, r#"version = "1.0.1""#);

        assert!(matches!(rechecksummed(&base, &lockfile("aa")), Ok(None)));
        assert!(matches!(rechecksummed(&base, &bumped), Ok(None)));
        assert!(matches!(
            rechecksummed(&base, &lockfile("bb")).as_ref().map(Option::as_deref),
            Ok(Some("serde@1.0.0"))
        ));
    }
}
