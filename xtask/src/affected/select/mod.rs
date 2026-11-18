//! The graph's half of the selection: changed paths mapped onto the workspace's package graph, the graph before the
//! change compared with the graph after it, and the reverse-dependency closure taken.

use crate::affected::base::BaseTree;
use crate::affected::simulation::Graphs;
use crate::affected::{lockfile, manifest, rules, test_targets};
use crate::failure::Failure;
use determinator::Determinator;
use guppy::graph::PackageGraph;
use std::collections::BTreeSet;
use std::fs;

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
    /// It changed this section of the root manifest, as its dotted key, which shapes how every package compiles and
    /// is not in the graph.
    Section(String),
    /// It changed the checksum of this package, `name@version`, under the same version and source: content no graph
    /// records.
    Checksum(String),
    /// It took this member out of the workspace. What the member's features unified into the others' builds is gone
    /// from the graph after the change, and determinator unifies the graph before it only with the members still in
    /// the graph after, so the diff cannot see it.
    Removed(String),
    /// What it reaches through the graph could not be worked out, for this reason: the graph before it, or a manifest
    /// or lockfile on either side, could not be read.
    Unknown(String),
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

/// The tests a change to `changed`, paths relative to the workspace's root, selects, `head` the graph after it and
/// `base` the tree before it.
///
/// Every package's where one path shapes every build or test ([`rules::screen`]). Otherwise, where the change could
/// reshape the graph ([`rules::reshaping`]), the graph before it is loaded from `base`, after the root manifest's
/// change is checked to be limited to what the graph holds ([`manifest`]) and the lockfile's to change no checksum
/// under one version ([`lockfile`]); any of those that fails selects every package. Then [`between`] the two graphs, or
/// at `head` alone where nothing could reshape it.
pub fn select(head: &PackageGraph, changed: &[String], base: &BaseTree) -> Result<Selection, Failure> {
    if changed.is_empty() {
        return Ok(Selection::Nothing);
    }
    if let Some(path) = rules::screen(changed) {
        return Ok(Selection::Every(Every::Shaping(path.to_owned())));
    }
    if !rules::reshaping(changed) {
        return between(Graphs::One(head), changed);
    }

    match before(head, changed, base) {
        Before::Graph(old) => between(Graphs::Two { old: &old, new: head }, changed),
        Before::Every(every) => Ok(Selection::Every(every)),
    }
}

/// What `changed` selects through its paths alone, with `graph` as both the graph before it and after it: what
/// [`select`] finds for a change that does not reshape the graph, and no more than it finds for one that does, which
/// adds what the graph diff reaches. The invariants ask this of a path, whatever a change to it would hold.
#[cfg(test)]
pub fn select_paths(graph: &PackageGraph, changed: &[String]) -> Result<Selection, Failure> {
    if changed.is_empty() {
        return Ok(Selection::Nothing);
    }
    if let Some(path) = rules::screen(changed) {
        return Ok(Selection::Every(Every::Shaping(path.to_owned())));
    }
    between(Graphs::One(graph), changed)
}

/// The graph before a change, or why every package is selected without one.
enum Before {
    Graph(Box<PackageGraph>),
    Every(Every),
}

/// The graph before a change that could reshape it, loaded from `base` once the root manifest and the lockfile, where
/// `changed` holds them, show nothing the graph diff cannot see; or why every package is selected without it.
fn before(head: &PackageGraph, changed: &[String], base: &BaseTree) -> Before {
    loaded(head, changed, base).unwrap_or_else(|failure| Before::Every(Every::Unknown(failure.message().to_owned())))
}

/// [`before`], failing where a file or the graph cannot be read.
fn loaded(head: &PackageGraph, changed: &[String], base: &BaseTree) -> Result<Before, Failure> {
    let root = head.workspace().root().as_std_path();
    let at_head = |path: &str| {
        fs::read_to_string(root.join(path))
            .map_err(|error| Failure::new(format_args!("failed head-file={path}: {error}")))
    };

    if holds(changed, rules::MANIFEST) {
        let unread = manifest::unread_change(&base.read(rules::MANIFEST)?, &at_head(rules::MANIFEST)?)?;
        if let Some(section) = unread {
            return Ok(Before::Every(Every::Section(section)));
        }
    }
    if holds(changed, rules::LOCKFILE) {
        let rechecksummed = lockfile::rechecksummed(&base.read(rules::LOCKFILE)?, &at_head(rules::LOCKFILE)?)?;
        if let Some(package) = rechecksummed {
            return Ok(Before::Every(Every::Checksum(package)));
        }
    }

    let old = base.graph()?;
    let removed = old
        .workspace()
        .iter()
        .map(|package| package.name())
        .find(|name| !head.workspace().contains_name(name))
        .map(str::to_owned);
    Ok(removed.map_or_else(|| Before::Graph(Box::new(old)), |name| Before::Every(Every::Removed(name))))
}

/// Whether `changed` holds `path`.
fn holds(changed: &[String], path: &str) -> bool {
    changed.iter().any(|candidate| candidate == path)
}

/// What `changed` selects at `graphs`.
///
/// Paths under a package's own `tests/` select that package and no dependent, past determinator ([`test_targets`]).
/// The rest go to determinator, under each simulation the graphs need ([`Graphs::reached`]): it applies
/// [`rules::determinator_rules`], maps each path to its nearest package, and closes over every package whose simulated
/// build, dev and build dependencies included, holds a changed one; and, given two graphs, it marks every member whose
/// simulated build holds a different set of packages, versions, sources, or features at one than at the other. A path
/// it can map to nothing marks every package. The [`WORKSPACE_SCANNERS`] are added to any selection that names a
/// package.
fn between(graphs: Graphs<'_>, changed: &[String]) -> Result<Selection, Failure> {
    let new = graphs.after();
    let split = test_targets::partition(new, changed);
    let mut packages = split.test_target_packages;
    packages.extend(graphs.reached(&split.graph_paths)?);
    if packages.is_empty() {
        return Ok(Selection::Nothing);
    }

    packages.extend(WORKSPACE_SCANNERS.iter().map(|name| (*name).to_owned()));
    if packages.len() == new.workspace().member_count() {
        return Ok(Selection::Every(Every::Reached));
    }
    Ok(Selection::Packages(packages))
}

/// A determinator over `old` and `new`, with [`rules::determinator_rules`] set.
pub fn determinator<'new, 'old>(
    old: &'old PackageGraph,
    new: &'new PackageGraph,
) -> Result<Determinator<'new, 'old>, Failure> {
    let mut determinator = Determinator::new(old, new);
    determinator.set_rules(&rules::determinator_rules(new)).map_err(Failure::new)?;
    Ok(determinator)
}

#[cfg(test)]
mod fixture;
#[cfg(test)]
mod tests;
