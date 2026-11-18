//! The workspace as the checks see it: its members, where each one's files are, and what a change to any path
//! selects, asked of the same function `cargo xtask affected` runs, so a check reports what the next pull request
//! would get.

use crate::affected::graph::Workspace;
use crate::affected::select::{self, Selection};
use crate::affected::{rules, test_targets};
use determinator::Determinator;
use determinator::rules::PathMatch;
use guppy::graph::{PackageGraph, PackageMetadata};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The loaded workspace.
pub struct Members {
    workspace: Workspace,
    root: PathBuf,
}

/// The selections already asked of a workspace. A check asks of thousands of paths, and a selection simulates every
/// member's build, so each is asked once per [`Attribution`]: a change to one path selects what its attribution does.
pub struct Selections<'graph> {
    determinator: Determinator<'graph, 'graph>,
    known: BTreeMap<Attribution, Selection>,
}

/// What a change to one path is attributed to, before any closure is taken: the same steps, in the same order, as
/// [`crate::affected::select::select`], whose result follows from this alone.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Attribution {
    /// The path is screened as shaping every build or test.
    Shaping(String),
    /// The path is under this member's own `tests/`.
    TestsOf(String),
    /// Determinator marks every package for it.
    Everything,
    /// Determinator marks these packages for it, none where a rule ignores it.
    Marked(BTreeSet<String>),
}

impl Members {
    pub fn load() -> Self {
        let Ok(workspace) = Workspace::load() else {
            panic!("the package graph loads");
        };
        let root = workspace.graph().workspace().root().as_std_path().to_path_buf();
        Self { workspace, root }
    }

    pub fn graph(&self) -> &PackageGraph {
        self.workspace.graph()
    }

    /// The workspace's root, absolute.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn iter(&self) -> impl Iterator<Item = PackageMetadata<'_>> {
        self.graph().workspace().iter()
    }

    /// `path` relative to the workspace's root, as a changed path is named.
    pub fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root).unwrap_or(path).display().to_string()
    }
}

impl<'graph> Selections<'graph> {
    pub fn new(members: &'graph Members) -> Self {
        let Ok(determinator) = select::determinator(members.graph(), members.graph()) else {
            panic!("the path rules apply");
        };
        Self { determinator, known: BTreeMap::new() }
    }

    /// What a change to `path` alone selects in `members`.
    pub fn of(&mut self, members: &Members, path: &Path) -> &Selection {
        let relative = members.relative(path);
        let changed = [relative];
        let attribution = if let Some(shaping) = rules::screen(&changed) {
            Attribution::Shaping(shaping.to_owned())
        } else if let Some(owner) = test_targets::partition(members.graph(), &changed).test_target_packages.pop_first()
        {
            Attribution::TestsOf(owner)
        } else {
            let mut marked = BTreeSet::new();
            let matched = self.determinator.match_path(&changed[0], |id| {
                marked.insert(id.repr().to_owned());
            });
            match matched {
                PathMatch::RuleMatchedAll | PathMatch::NoMatches => Attribution::Everything,
                PathMatch::RuleMatched(_) | PathMatch::AncestorMatched => Attribution::Marked(marked),
            }
        };

        self.known.entry(attribution).or_insert_with(|| {
            let Ok(selection) = members.workspace.select_paths(&changed) else {
                panic!("a selection is computed for {}", changed[0]);
            };
            selection
        })
    }
}

/// The directory `package`'s manifest is in.
pub fn package_root(package: PackageMetadata<'_>) -> &Path {
    package.manifest_path().parent().map_or_else(|| Path::new(""), |root| root.as_std_path())
}
