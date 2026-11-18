//! The one narrowing below package granularity: a package's own integration tests have no dependents.
//!
//! A file under `<package root>/tests/` compiles into that package's integration-test targets and nothing else. Nothing
//! links against a test binary, so the file cannot change what a dependent builds, and its reverse-dependency closure
//! is empty. Handed to determinator, it would mark the package changed and select every dependent too, whose tests it
//! provably cannot reach; so such paths are split out here and select their package alone.
//!
//! The match is narrow on purpose. Only the `tests/` directly under a member's root counts, read from guppy's own
//! workspace paths rather than a `crates/*/tests/` glob: a `src/**/tests/` directory or `tests.rs` file is a unit-test
//! module compiled into the library, and keeps its closure. That `tests/` feeds nothing but test targets is checked by
//! the `test_isolation` and `references` invariants.

use guppy::graph::PackageGraph;
use std::collections::{BTreeMap, BTreeSet};

/// The directory, relative to a package's root, cargo finds its integration-test targets in.
pub const TEST_DIR: &str = "tests";

/// Changed paths, split by whether they lie in a member's own integration tests.
pub struct Partition<'changed> {
    /// The members owning a changed path under their `tests/`: each selects itself, and no dependent.
    pub test_target_packages: BTreeSet<String>,
    /// Every other changed path, for determinator to map.
    pub graph_paths: Vec<&'changed str>,
}

/// Splits `changed` into the paths under a member's own `tests/` and the rest.
///
/// A member whose root is the workspace's root is skipped: none is, and one that were would claim a top-level `tests/`.
pub fn partition<'changed>(graph: &PackageGraph, changed: &'changed [String]) -> Partition<'changed> {
    let roots: BTreeMap<String, &str> = graph
        .workspace()
        .iter()
        .filter_map(|package| {
            let root = package.source().workspace_path()?.as_str();
            (!root.is_empty()).then(|| (format!("{root}/{TEST_DIR}/"), package.name()))
        })
        .collect();

    let mut test_target_packages = BTreeSet::new();
    let mut graph_paths = Vec::new();
    for path in changed.iter().map(String::as_str) {
        match roots.iter().find(|(prefix, _)| path.starts_with(prefix.as_str())) {
            Some((_, owner)) => {
                test_target_packages.insert((*owner).to_owned());
            }
            None => graph_paths.push(path),
        }
    }
    Partition { test_target_packages, graph_paths }
}

#[cfg(test)]
mod tests {
    use super::partition;
    use crate::affected::graph::Workspace;
    use std::collections::BTreeSet;

    #[test]
    fn only_a_member_s_own_tests_directory_splits_out() {
        // Catches a prefix loose enough to claim a unit-test module compiled into a library (`src/**/tests.rs`), which
        // would drop the closure of a change every dependent compiles, and a guest crate nested in a member, whose
        // `tests/`-free files must reach determinator.
        let Ok(workspace) = Workspace::load() else {
            panic!("the package graph loads");
        };
        let changed: Vec<String> = [
            "crates/aethereal-chassis-headless/tests/library.rs",
            "crates/aethereal-ui-macros/tests/refusals/a.stderr",
            "crates/aethereal-render/src/font/tests.rs",
            "crates/aethereal-chassis-headless/guests/echo/src/lib.rs",
            "docs/adr/0001-entry.md",
        ]
        .map(String::from)
        .to_vec();
        let split = partition(workspace.graph(), &changed);

        let owners: BTreeSet<String> =
            ["aethereal-chassis-headless", "aethereal-ui-macros"].map(String::from).into_iter().collect();
        assert_eq!(split.test_target_packages, owners);
        assert_eq!(
            split.graph_paths,
            [
                "crates/aethereal-render/src/font/tests.rs",
                "crates/aethereal-chassis-headless/guests/echo/src/lib.rs",
                "docs/adr/0001-entry.md"
            ]
        );
    }
}
