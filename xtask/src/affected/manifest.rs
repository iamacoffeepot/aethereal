//! The root manifest's change, sorted by section: the dependency and member lists, whose change the graph diff reads,
//! and every other section, which shapes how every package compiles without reshaping the graph.
//!
//! The root manifest holds two kinds of thing. `[workspace.dependencies]` and the member lists say which packages are
//! built and from what: a change there reaches a member only through the package graph, which `cargo metadata` resolves
//! from them, so comparing the graph before the change with the graph after it finds every member it reaches. The rest,
//! `[profile.*]`, `[patch]`, `[replace]`, `[workspace.lints]`, `[workspace.package]`, the resolver, changes how every
//! package compiles, or what it inherits, and the graph does not show it. The two manifests are parsed and compared key
//! by key, so a comment or a reordering changes nothing; a section this module does not name counts as the second kind.

use crate::failure::Failure;
use std::collections::BTreeSet;
use toml::{Table, Value};

/// The keys of `[workspace]` whose change the graph diff reads: the dependencies members inherit, and which packages
/// are members.
///
/// `default-members` is among them: it names what a cargo command that names no package builds, and every command CI
/// runs names `--workspace` or `-p`, so its change reaches no build CI runs. It changes with `members` whenever a crate
/// enters, so leaving it out would run every test for every new crate.
const GRAPH_KEYS: &[&str] = &["dependencies", "members", "exclude", "default-members"];

/// The table every [`GRAPH_KEYS`] entry sits in.
const WORKSPACE: &str = "workspace";

/// The first section that differs between the root manifests `base` and `head` and is not one the graph diff reads, as
/// its dotted key (`profile`, `workspace.lints`), where one does.
///
/// A manifest that does not parse fails: what its change reaches cannot be told.
pub fn unread_change(base: &str, head: &str) -> Result<Option<String>, Failure> {
    let base: Table = base.parse().map_err(|error| Failure::new(format_args!("failed base-manifest: {error}")))?;
    let head: Table = head.parse().map_err(|error| Failure::new(format_args!("failed head-manifest: {error}")))?;

    for key in keys(&base, &head).into_iter().filter(|key| base.get(*key) != head.get(*key)) {
        if key != WORKSPACE {
            return Ok(Some(key.to_owned()));
        }

        // A `workspace` that is not a table on both sides is no manifest cargo reads, and is left to every package.
        let (Some(Value::Table(base)), Some(Value::Table(head))) = (base.get(key), head.get(key)) else {
            return Ok(Some(key.to_owned()));
        };
        let unread = keys(base, head).into_iter().find(|inner| changed_unread(inner, base, head));
        if let Some(inner) = unread {
            return Ok(Some(format!("{WORKSPACE}.{inner}")));
        }
    }
    Ok(None)
}

/// Whether `[workspace]`'s `key` differs between `base` and `head` and is not one the graph diff reads.
fn changed_unread(key: &str, base: &Table, head: &Table) -> bool {
    let changed = base.get(key) != head.get(key);
    let unread = !GRAPH_KEYS.contains(&key);
    changed && unread
}

/// Every key either table holds, each once, in order.
fn keys<'table>(base: &'table Table, head: &'table Table) -> BTreeSet<&'table str> {
    base.keys().chain(head.keys()).map(String::as_str).collect()
}

#[cfg(test)]
mod tests {
    use super::unread_change;

    const BASE: &str = r#"
[workspace]
resolver = "3"
members = ["crates/a"]

[workspace.dependencies]
syn = "2"

[workspace.lints.clippy]
pedantic = "warn"

[profile.release]
lto = "fat"
"#;

    /// What a change from [`BASE`] to `head` leaves to every package.
    fn unread(head: &str) -> Option<String> {
        let Ok(unread) = unread_change(BASE, head) else {
            panic!("both manifests parse");
        };
        unread
    }

    #[test]
    fn only_the_dependency_and_member_lists_go_to_the_graph() {
        // Catches a section that shapes every build waved through to the graph diff, which sees nothing of a profile,
        // a lint, or the resolver, and a dependency or member change, or a comment, sent to every package.
        let graph_only = BASE.replace(r#"syn = "2""#, "syn = { version = \"2\", features = [\"full\"] }\n# a note");
        let new_member = BASE.replace(r#"["crates/a"]"#, r#"["crates/a", "crates/b"]"#);
        assert_eq!(unread(&graph_only), None);
        assert_eq!(unread(&new_member), None);

        assert_eq!(unread(&BASE.replace("fat", "thin")).as_deref(), Some("profile"));
        assert_eq!(unread(&BASE.replace(r#""warn""#, r#""deny""#)).as_deref(), Some("workspace.lints"));
        assert_eq!(unread(&BASE.replace(r#""3""#, r#""2""#)).as_deref(), Some("workspace.resolver"));
        assert_eq!(
            unread(&format!("{BASE}\n[patch.crates-io]\nsyn = {{ path = \"syn\" }}\n")).as_deref(),
            Some("patch")
        );
    }
}
