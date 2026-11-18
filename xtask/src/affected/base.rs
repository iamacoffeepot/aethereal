//! The tree a change is diffed against, read as the change's own files are: a file by its path, and its package graph
//! from a checkout of it.
//!
//! The graph comes from `cargo metadata` run in a temporary git worktree of the base revision, under the system's
//! temporary directory, removed once the graph is read. `cargo metadata` needs the base's whole manifest tree and the
//! files each target is found by, which a worktree holds as the revision does; it reads the base's own lockfile under
//! `--locked`, so a lockfile that does not resolve fails rather than being resolved anew into a graph the base never had.

use super::git;
use crate::failure::Failure;
use guppy::MetadataCommand;
use guppy::graph::PackageGraph;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::{env, fs};

/// A revision of the repository at `root`, as the base of a change.
pub struct BaseTree {
    root: PathBuf,
    revision: String,
}

impl BaseTree {
    /// The revision `revision` names in the repository at `root`.
    pub fn new(root: &Path, revision: &str) -> Self {
        Self { root: root.to_path_buf(), revision: revision.to_owned() }
    }

    /// The file at `path`, relative to the repository's root, as the revision holds it.
    pub fn read(&self, path: &str) -> Result<String, Failure> {
        git(&self.root, &["show", &format!("{}:{path}", self.revision)])
    }

    /// The package graph `cargo metadata` describes from a checkout of the revision.
    pub fn graph(&self) -> Result<PackageGraph, Failure> {
        let commit = git(&self.root, &["rev-parse", "--verify", &format!("{}^{{commit}}", self.revision)])?;
        let checkout = Checkout::add(&self.root, commit.trim())?;
        MetadataCommand::new()
            .current_dir(&checkout.directory)
            .other_options(["--locked"])
            .build_graph()
            .map_err(|error| Failure::new(format_args!("failed base-graph: {error}")))
    }
}

/// A temporary worktree of one commit, removed when dropped.
struct Checkout {
    root: PathBuf,
    directory: PathBuf,
}

impl Checkout {
    /// A worktree of `commit` of the repository at `root`, under the system's temporary directory, named by this
    /// process and the commit so that two runs, or two repositories' tests in one run, never share one.
    fn add(root: &Path, commit: &str) -> Result<Self, Failure> {
        let directory = env::temp_dir().join(format!("aethereal-affected-base-{}-{commit}", process::id()));
        let _ = fs::remove_dir_all(&directory);
        let path = directory.to_str().ok_or_else(|| Failure::new("failed base-checkout: the path is not UTF-8"))?;
        git(root, &["worktree", "add", "--detach", "--quiet", path, commit])?;
        Ok(Self { root: root.to_path_buf(), directory })
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        let removed = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&self.directory)
            .current_dir(&self.root)
            .output()
            .is_ok_and(|output| output.status.success());
        if !removed {
            let _ = fs::remove_dir_all(&self.directory);
            let _ = Command::new("git").args(["worktree", "prune"]).current_dir(&self.root).output();
        }
    }
}
