//! The workspace's package graph, read once from `cargo metadata` and asked what a change selects.

use crate::affected::base::BaseTree;
use crate::affected::select::{self, Selection};
use crate::cargo;
use crate::failure::Failure;
use guppy::MetadataCommand;
use guppy::graph::{BuildTargetId, PackageGraph};
use std::collections::BTreeSet;
use std::path::Path;

/// The workspace's package graph.
pub struct Workspace {
    graph: PackageGraph,
}

impl Workspace {
    /// The graph `cargo metadata` describes from the workspace's root.
    pub fn load() -> Result<Self, Failure> {
        Self::load_at(cargo::root())
    }

    /// The graph `cargo metadata` describes from `root`.
    pub fn load_at(root: &Path) -> Result<Self, Failure> {
        let graph = MetadataCommand::new()
            .current_dir(root)
            .build_graph()
            .map_err(|error| Failure::new(format_args!("failed package-graph: {error}")))?;
        Ok(Self { graph })
    }

    /// The tests a change to `changed`, relative to the workspace's root, from `base`, selects through the graph.
    pub fn select(&self, changed: &[String], base: &BaseTree) -> Result<Selection, Failure> {
        select::select(&self.graph, changed, base)
    }

    /// What `changed` selects through its paths alone, as the invariants ask it ([`select::select_paths`]).
    #[cfg(test)]
    pub fn select_paths(&self, changed: &[String]) -> Result<Selection, Failure> {
        select::select_paths(&self.graph, changed)
    }

    /// The members with a library target: the only ones with doctests, and the only ones `cargo test --doc` takes by
    /// name, as it refuses a `-p` that names none.
    pub fn libraries(&self) -> BTreeSet<String> {
        self.graph
            .workspace()
            .iter()
            .filter(|package| package.build_targets().any(|target| target.id() == BuildTargetId::Library))
            .map(|package| package.name().to_owned())
            .collect()
    }

    /// The graph itself, for the invariants that check what it cannot see.
    #[cfg(test)]
    pub const fn graph(&self) -> &PackageGraph {
        &self.graph
    }
}
