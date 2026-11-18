//! Members inside another member's directory: whatever rebuilds one selects what a change to its enclosing member does.
//!
//! The headless chassis's guests (`crates/aethereal-chassis-headless/guests/echo`) are crates its build script builds
//! for wasm32, on every build of the chassis, its dependents' builds included; its tests run the blobs. Cargo resolves
//! the guest as a member, through the chassis's path dev-dependency on it, but a dev-dependency edge reaches the
//! chassis's tests alone: a change that fails the guest's build fails every build of the chassis, its dependents'
//! included. So a change that rebuilds a nested member must select its enclosing member's whole closure.
//!
//! Two kinds of change rebuild one: a change to its own files, which [`crate::affected::rules::determinator_rules`]
//! marks the enclosing member for, and a change to a member it is built from, which must reach the enclosing member's
//! dependents through the graph, as `aethereal-actor` and `aethereal-data` do because the chassis depends on them too.

use crate::affected::invariants::source;
use crate::affected::invariants::workspace::{Members, Selections, package_root};
use guppy::graph::{DependencyDirection, PackageMetadata};
use std::iter;

#[test]
fn a_change_that_rebuilds_a_nested_member_selects_what_its_enclosing_member_s_change_does() {
    // Catches a guest that depends on a member its chassis does not, whose change reaches the chassis's tests through
    // the guest's dev-dependency edge and none of the chassis's dependents, and a guest's own files left to that edge.
    let members = Members::load();
    let mut selections = Selections::new(&members);
    let mut violations = Vec::new();
    for inner in members.iter() {
        let inner_root = package_root(inner);
        let enclosing =
            members.iter().filter(|outer| outer.id() != inner.id() && inner_root.starts_with(package_root(*outer)));
        for outer in enclosing {
            let needed = selections.of(&members, outer.manifest_path().as_std_path()).clone();
            let Ok(query) = members.graph().query_forward(iter::once(inner.id())) else {
                panic!("the graph answers what {} depends on", inner.name());
            };
            let built_from: Vec<PackageMetadata<'_>> = query
                .resolve()
                .packages(DependencyDirection::Forward)
                .filter(|package| package.in_workspace() && package.id() != inner.id())
                .collect();

            let changes = source::walk(inner_root)
                .files
                .into_iter()
                .chain(built_from.iter().map(|package| package.manifest_path().as_std_path().to_path_buf()));
            for changed in changes {
                let got = selections.of(&members, &changed);
                if !got.covers(&needed) {
                    violations.push(format!(
                        "  {inner} sits inside {outer}, which builds it. A change to {changed} rebuilds {inner} and \
                         selects {got:?}; a change to {outer} selects {needed:?}.\n    Fix: make {outer} depend on \
                         what {inner} is built from, as the headless chassis depends on aethereal-actor.",
                        inner = inner.name(),
                        outer = outer.name(),
                        changed = members.relative(&changed),
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "a nested member's change selects less than its builder's:\n\n{}\n",
        violations.join("\n\n")
    );
}
