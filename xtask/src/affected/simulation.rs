//! The builds determinator simulates for every member, at the graph before a change and at the graph after it, to find
//! the members whose build the change reaches.
//!
//! Determinator does two things with a simulated build of each member, dev-dependencies included. It takes the reverse
//! closure of a changed path over the packages the build holds; and, given two graphs, it marks the member changed when
//! its build holds a different set of packages, versions, sources, or enabled features at one than at the other. One
//! simulation can hide a feature change another shows, as a feature already on in a wider unification hides one turned
//! on in a narrower, so two graphs are compared under four, each feature resolver × each unification, and the union is
//! taken:
//!
//! - **Resolver**: determinator's default, cargo's V1, and V3, the one the root manifest names and cargo builds with.
//!   V1 unifies features across host and target, dev-dependencies, and inactive platforms, where V3 keeps them apart.
//! - **Unification**: a member built alone, as a pull request's `-p` selection builds it with few others; and with every
//!   member, every feature on, as `cargo nextest run --workspace --all-features` builds it on `main`, where a feature
//!   one member turns on in a shared dependency reaches every member that builds that dependency.
//!
//! With one graph, as both the before and the after, no build differs, and only the closure is asked. A wider
//! unification only turns features on, and V1 turns on every feature V3 does, so the packages a build holds only grow
//! from V3 to V1 and from alone to the workspace: the widest simulation's closure holds every other's, and it alone is
//! run. It also holds what a member built alone does not: a member's optional dependency, which another member turns on
//! through a shared intermediate, is in the workspace's build of every member that builds that intermediate.

use crate::affected::select;
use crate::failure::Failure;
use determinator::Determinator;
use guppy::graph::cargo::CargoResolverVersion;
use guppy::graph::feature::StandardFeatures;
use guppy::graph::{DependencyDirection, PackageGraph, PackageMetadata};
use std::collections::BTreeSet;

/// The graphs a change is simulated at.
#[derive(Clone, Copy)]
pub enum Graphs<'graph> {
    /// One graph, as both the before and the after: nothing the change holds could reshape it.
    One(&'graph PackageGraph),
    /// The graph before the change and the graph after it.
    Two { old: &'graph PackageGraph, new: &'graph PackageGraph },
}

impl<'graph> Graphs<'graph> {
    /// The graph after the change.
    pub const fn after(self) -> &'graph PackageGraph {
        match self {
            Self::One(graph) | Self::Two { new: graph, .. } => graph,
        }
    }

    /// The graph before the change.
    const fn before(self) -> &'graph PackageGraph {
        match self {
            Self::One(graph) | Self::Two { old: graph, .. } => graph,
        }
    }

    /// The members a change to `paths` reaches at these graphs, the union over every simulation they need: the members
    /// `paths` map to and every member building one of them, and every member whose simulated build differs between
    /// the two graphs.
    pub fn reached(self, paths: &[&str]) -> Result<BTreeSet<String>, Failure> {
        let simulations: &[Simulation] = match self {
            Self::One(_) => &[WIDEST],
            Self::Two { .. } => &SIMULATIONS,
        };

        let mut reached = BTreeSet::new();
        for simulation in simulations {
            reached.extend(simulation.reached(self.before(), self.after(), paths)?);
        }
        Ok(reached)
    }
}

/// One build of every member that determinator simulates.
#[derive(Clone, Copy)]
struct Simulation {
    resolver: CargoResolverVersion,
    unification: Unification,
}

/// Which members' features a simulated build of one member unifies with.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Unification {
    /// Its own alone.
    Alone,
    /// Every member's, every feature on.
    Workspace,
}

/// The simulation whose builds hold the most packages, whose closure holds every other's.
const WIDEST: Simulation = Simulation { resolver: CargoResolverVersion::V1, unification: Unification::Workspace };

/// Every simulation two graphs are compared under.
const SIMULATIONS: [Simulation; 4] = [
    Simulation { resolver: CargoResolverVersion::V1, unification: Unification::Alone },
    WIDEST,
    Simulation { resolver: CargoResolverVersion::V3, unification: Unification::Alone },
    Simulation { resolver: CargoResolverVersion::V3, unification: Unification::Workspace },
];

impl Simulation {
    /// The members a change reaches under this simulation, `old` the graph before it and `new` the graph after.
    fn reached(self, old: &PackageGraph, new: &PackageGraph, paths: &[&str]) -> Result<BTreeSet<String>, Failure> {
        let mut options = Determinator::default_cargo_options();
        options.set_resolver(self.resolver);

        let mut determinator = select::determinator(old, new)?;
        determinator.set_cargo_options(&options);
        if self.unification == Unification::Workspace {
            let members = new.workspace().iter().map(|package| package.name());
            determinator.set_features_only(members, StandardFeatures::All).map_err(Failure::new)?;
        }
        determinator.add_changed_paths(paths.iter().copied());

        Ok(determinator
            .compute()
            .affected_set
            .packages(DependencyDirection::Forward)
            .filter(PackageMetadata::in_workspace)
            .map(|package| package.name().to_owned())
            .collect())
    }
}
