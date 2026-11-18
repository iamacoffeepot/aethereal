//! The graph's half of the selection: changed paths mapped onto the workspace's package graph, and the
//! reverse-dependency closure taken.

use crate::affected::{rules, test_targets};
use crate::failure::Failure;
use determinator::Determinator;
use guppy::graph::{DependencyDirection, PackageGraph, PackageMetadata};
use std::collections::BTreeSet;

/// The packages whose tests read every package's sources, so a change to any package can fail them though no
/// dependency edge leads back: xtask, whose `affected` invariants scan every package's sources and manifests for what
/// the graph cannot see. Every selection that names a package names these too.
pub const WORKSPACE_SCANNERS: &[&str] = &[env!("CARGO_PKG_NAME")];

/// The tests a change selects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// Every package's.
    Every(Every),
    /// These packages', never none.
    Packages(BTreeSet<String>),
    /// No package's: the change reaches no build and no test.
    Nothing,
}

/// Why a change selects every package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Every {
    /// It changed this path, which shapes every build or test.
    Shaping(String),
    /// It reaches every package through the graph, or changed a path no package and no rule claims.
    Reached,
}

impl Selection {
    /// Whether this runs every test `other` runs.
    #[cfg(test)]
    pub fn covers(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Every(_), _) | (_, Self::Nothing) => true,
            (Self::Packages(packages), Self::Packages(needed)) => needed.is_subset(packages),
            (Self::Packages(_) | Self::Nothing, Self::Every(_)) | (Self::Nothing, Self::Packages(_)) => false,
        }
    }
}

/// The tests a change to `changed`, paths relative to the workspace's root, selects: every package's where one path
/// shapes every build or test ([`rules::screen`]); otherwise `changed` mapped onto `graph` and the reverse-dependency
/// closure taken, with the [`WORKSPACE_SCANNERS`] added to any selection that names a package.
///
/// Paths under a package's own `tests/` select that package and no dependent, past determinator ([`test_targets`]).
/// The rest go to determinator, which applies [`rules::determinator_rules`], maps each path to its nearest package,
/// and closes over every package whose simulated build, dev and build dependencies and every feature included, holds a
/// changed one. A path it can map to nothing marks every package.
///
/// One graph is handed to determinator as both the old and the new: a path that could reshape it has already selected
/// every package.
pub fn select(graph: &PackageGraph, changed: &[String]) -> Result<Selection, Failure> {
    if changed.is_empty() {
        return Ok(Selection::Nothing);
    }
    if let Some(path) = rules::screen(changed) {
        return Ok(Selection::Every(Every::Shaping(path.to_owned())));
    }

    let split = test_targets::partition(graph, changed);
    let mut determinator = determinator(graph)?;
    determinator.add_changed_paths(split.graph_paths.iter().copied());

    let mut packages: BTreeSet<String> = determinator
        .compute()
        .affected_set
        .packages(DependencyDirection::Forward)
        .filter(PackageMetadata::in_workspace)
        .map(|package| package.name().to_owned())
        .collect();
    packages.extend(split.test_target_packages);
    if packages.is_empty() {
        return Ok(Selection::Nothing);
    }

    packages.extend(WORKSPACE_SCANNERS.iter().map(|name| (*name).to_owned()));
    if packages.len() == graph.workspace().member_count() {
        return Ok(Selection::Every(Every::Reached));
    }
    Ok(Selection::Packages(packages))
}

/// A determinator over `graph`, as both its old and its new, with [`rules::determinator_rules`] set.
pub fn determinator(graph: &PackageGraph) -> Result<Determinator<'_, '_>, Failure> {
    let mut determinator = Determinator::new(graph, graph);
    determinator.set_rules(&rules::determinator_rules(graph)).map_err(Failure::new)?;
    Ok(determinator)
}
