//! Where targets compile from: the premises of attributing a changed path to a package by its directory, and of
//! narrowing a change under `tests/` to its own package.
//!
//! [`crate::affected::test_targets`] selects a package alone, no dependent, for a change under its `tests/`, on the
//! premise that only test targets compile from there, and determinator attributes a changed file to the member whose
//! directory holds it. The `references` check covers what a source reads; these cover what cargo compiles: a manifest's
//! `path = …` can point a library or a binary at `tests/` or outside the package, and a symlink can alias one tree into
//! another, leaving no text for any check to follow.

use crate::affected::invariants::source;
use crate::affected::invariants::workspace::{Members, package_root};
use crate::affected::test_targets::TEST_DIR;
use guppy::graph::BuildTargetId;

#[test]
fn every_target_compiles_from_inside_its_own_package_and_only_tests_from_tests() {
    // Catches a `[[bin]]` or `[lib]` pointed into `tests/`, which makes a change there change what dependents build
    // while the narrowing drops them, and a target compiled from another package's directory, which a change there
    // attributes to that package instead.
    let members = Members::load();
    let mut violations = Vec::new();
    for package in members.iter() {
        let root = package_root(package);
        let tests = root.join(TEST_DIR);
        for target in package.build_targets() {
            let path = target.path().as_std_path();
            let name = package.name();
            let outside = !path.starts_with(root);
            let not_a_test = !matches!(target.id(), BuildTargetId::Test(_));
            let under_tests = path.starts_with(&tests) && not_a_test;
            if outside {
                violations.push(format!(
                    "  {name}'s target {:?} compiles from {}, outside its package: a change there selects the \
                     package whose directory holds it.",
                    target.id(),
                    members.relative(path),
                ));
            } else if under_tests {
                violations.push(format!(
                    "  {name}'s target {:?} compiles from {}, under its `tests/`: only test targets may, as a change \
                     there selects {name} alone and none of its dependents.",
                    target.id(),
                    members.relative(path),
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "a target compiles from where its changes are misattributed:\n\n{}\n",
        violations.join("\n\n")
    );
}

#[test]
fn no_symlink_sits_inside_a_member() {
    // Catches a symlink aliasing a `tests/` file into a library, or one package's files under another's root, which
    // leaves no text for the other checks to follow.
    let members = Members::load();
    let links: Vec<String> = members
        .iter()
        .flat_map(|package| source::walk(package_root(package)).symlinks)
        .map(|link| format!("  {}", members.relative(&link)))
        .collect();

    assert!(
        links.is_empty(),
        "a symlink sits inside a member, where no check can follow what it aliases:\n\n{}\n\n  Fix: replace it with \
         the file, or move what it shares into the package that needs it.\n",
        links.join("\n")
    );
}
