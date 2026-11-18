//! `cargo xtask affected`: the packages whose tests a change can fail, so that a pull request's CI runs theirs and not
//! the whole workspace's.
//!
//! It diffs the checkout against a base, maps each changed path onto the workspace's package graph (guppy and
//! determinator), and takes the reverse-dependency closure: the package a path sits in, and every package that builds on
//! it through any dependency, dev and build ones included, with every feature on. Where the change touches a manifest or
//! the lockfile, it also compares the package graph at the base with the graph at the head, and adds every member whose
//! simulated build the change reaches: a dependency's version, source, or features, or an edge, changed.
//!
//! ```text
//! git diff <base> HEAD ──> changed paths ──> rules::screen ──> a hit: every package
//!                                                  │
//!                                                  ├─> a manifest or Cargo.lock changed: the base's graph, from a
//!                                                  │   worktree of it ([`base`]), once the root manifest's change is
//!                                                  │   limited to its dependency and member lists ([`manifest`]) and
//!                                                  │   no checksum changed under one version ([`lockfile`]); any of
//!                                                  │   those that fails: every package
//!                                                  │
//!                                                  └─> test_targets::partition ──> under a package's `tests/`: that package
//!                                                           │
//!                                                           └─> determinator, base graph against head graph, under
//!                                                               each [`simulation`]: path rules, then the nearest
//!                                                               package, then every package building on it; and every
//!                                                               member whose simulated build differs between the two
//! ```
//!
//! **A missed package is the one failure this must never have.** A slow suite costs minutes; a miss costs a bisect. So
//! the selection is conservative by construction:
//!
//! - a path that shapes the toolchain or how the tests run selects every package ([`rules`]), as does a root manifest
//!   section the graph does not hold ([`manifest`]);
//! - a base graph that cannot be loaded, a manifest or lockfile that cannot be read, a checksum changed under one
//!   version, or a member taken out of the workspace selects every package ([`select`]);
//! - a changed path that no package and no rule claims selects every package (determinator's own fallback);
//! - a push to `main` runs every test whatever changed, so a miss here delays a red signal to the merge, never loses it.
//!
//! The one narrowing below package granularity is [`test_targets`]: a path under a package's own `tests/` compiles into
//! an integration-test binary nothing links against, so it selects that package and no dependent.
//!
//! What cargo's graph cannot see, a file one package reads by path from another, a guest crate one package builds, is
//! checked by the test-only `invariants`, which every narrowed selection runs: xtask is selected with any package.

mod base;
pub mod graph;
#[cfg(test)]
mod invariants;
mod lockfile;
mod manifest;
mod rules;
pub mod select;
mod simulation;
mod test_targets;

use crate::cargo;
use crate::check::{self, Job, Tests};
use crate::failure::Failure;
use base::BaseTree;
use select::{Every, Selection};
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::Path;
use std::process::Command;
use std::{env, str};

/// What `cargo xtask affected` is asked.
pub struct Request {
    /// What the checkout is diffed against.
    pub base: Base,
    /// Whether the selection is also written to the step-output file GitHub Actions names in `GITHUB_OUTPUT`.
    pub github_output: bool,
    /// Whether the selected tests are run, as CI's test job runs them.
    pub run: bool,
}

/// What the checkout is diffed against.
pub enum Base {
    /// Where `HEAD` left `origin/main`: their merge base. A local run's default.
    MergeBase,
    /// A ref named: CI names `HEAD^1`, which on a pull request's merge commit is the tip of the branch it merges into.
    Named(String),
}

/// Diffs the checkout against its base, prints the selection, writes it for CI when asked, and runs it when asked.
pub fn run(request: &Request) -> Result<(), Failure> {
    let root = cargo::root();
    let base = match &request.base {
        Base::MergeBase => git(root, &["merge-base", "origin/main", "HEAD"])?.trim().to_owned(),
        Base::Named(base) => base.clone(),
    };

    let changed = changed_paths(root, &base, "HEAD")?;
    let selection = graph::Workspace::load()?.select(&changed, &BaseTree::new(root, &base))?;
    println!("{}", report(&selection, changed.len()));
    if request.github_output {
        write_github_output(&selection)?;
    }

    if !request.run {
        return Ok(());
    }
    match selection {
        Selection::Every(_) => check::run(&[Job::Test], &Tests::Workspace),
        Selection::Packages(packages) => check::run(&[Job::Test], &Tests::Packages(packages)),
        Selection::Nothing => Ok(()),
    }
}

/// The paths that differ between the trees `from` and `to` name, relative to the repository's root.
///
/// Renames are listed as the path they left and the path they reached: git's default detection lists only the second,
/// and a file moved out of a package changes that package too.
fn changed_paths(root: &Path, from: &str, to: &str) -> Result<Vec<String>, Failure> {
    let diff = git(root, &["diff", "--name-only", "--no-renames", "-z", from, to])?;
    Ok(diff.split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect())
}

/// The line printed for a selection: its status, then `key=value` pairs, each value one token.
fn report(selection: &Selection, changed: usize) -> String {
    match selection {
        Selection::Every(Every::Shaping(path)) => format!("selected tests=all changed={changed} shaping={path}"),
        Selection::Every(Every::Section(section)) => {
            format!("selected tests=all changed={changed} shaping=Cargo.toml section={section}")
        }
        Selection::Every(Every::Checksum(package)) => {
            format!("selected tests=all changed={changed} shaping=Cargo.lock checksum={package}")
        }
        Selection::Every(Every::Removed(member)) => format!("selected tests=all changed={changed} removed={member}"),
        Selection::Every(Every::Unknown(reason)) => {
            format!("selected tests=all changed={changed} reached=unknown\n{reason}")
        }
        Selection::Every(Every::Reached) => format!("selected tests=all changed={changed} reached=every-package"),
        Selection::Packages(packages) => {
            let names: Vec<&str> = packages.iter().map(String::as_str).collect();
            format!("selected tests={} changed={changed} packages={}", names.len(), names.join(","))
        }
        Selection::Nothing => format!("selected tests=none changed={changed} reached=no-package"),
    }
}

/// Appends the selection to the step-output file CI reads as `steps.<id>.outputs.*`: `run_all`, `packages`, and
/// `package_args`, the `-p <package>` flags `cargo xtask check test` takes.
fn write_github_output(selection: &Selection) -> Result<(), Failure> {
    let path = env::var_os("GITHUB_OUTPUT").ok_or_else(|| {
        Failure::new("refused --github-output: GitHub Actions names the step-output file in GITHUB_OUTPUT")
    })?;

    let mut lines = String::new();
    let run_all = matches!(selection, Selection::Every(_));
    let packages: Vec<&str> = match selection {
        Selection::Packages(packages) => packages.iter().map(String::as_str).collect(),
        Selection::Every(_) | Selection::Nothing => Vec::new(),
    };
    let package_args: Vec<String> = packages.iter().map(|name| format!("-p {name}")).collect();
    let _ = writeln!(lines, "run_all={run_all}");
    let _ = writeln!(lines, "packages={}", packages.join(" "));
    let _ = writeln!(lines, "package_args={}", package_args.join(" "));

    OpenOptions::new().append(true).open(path)?.write_all(lines.as_bytes())?;
    Ok(())
}

/// What `git <args>`, run in `root`, prints. One that does not succeed fails with what git printed.
fn git(root: &Path, args: &[&str]) -> Result<String, Failure> {
    let output = Command::new("git").args(args).current_dir(root).output()?;
    let failed = !output.status.success();
    if failed {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Failure::new(format_args!("failed git={} {}", args.join("-"), stderr.trim())));
    }

    String::from_utf8(output.stdout).map_err(|_| Failure::new(format_args!("failed git={}: not UTF-8", args.join("-"))))
}

#[cfg(test)]
mod tests;
