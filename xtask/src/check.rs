//! What CI runs, one list for CI and a local run: each of CI's jobs calls `cargo xtask check <job>`, and a bare
//! `cargo xtask check` runs every job, in order, stopping at the first that fails.
//!
//! The test job runs every package's tests, or, given `-p <package>` flags, those packages' alone: a pull request's CI
//! hands it what `cargo xtask affected` selects, and a push to `main` runs it bare.

use crate::affected::graph::Workspace;
use crate::cargo;
use crate::failure::Failure;
use std::collections::BTreeSet;

/// One of CI's jobs, as the cargo commands it runs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Job {
    /// Format: the code is as rustfmt writes it.
    Fmt,
    /// Clippy: the workspace linted for the machine it is built on.
    Clippy,
    /// Browser host: the workspace linted for wasm32, where a browser's host itself runs (ADR-0001).
    Browser,
    /// Test: the tests, then the doctests, of the packages [`Tests`] names.
    Test,
}

/// Whose tests the test job runs.
pub enum Tests {
    /// Every package's: a bare `check`, and every push to `main`.
    Workspace,
    /// These packages', never none: what `cargo xtask affected` selects for a pull request.
    Packages(BTreeSet<String>),
}

impl Job {
    /// Every job, in the order a bare `check` runs them: the lints first, as they fail soonest.
    pub const ALL: [Self; 4] = [Self::Fmt, Self::Clippy, Self::Browser, Self::Test];

    /// The job a command line names.
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|job| job.name() == name)
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Fmt => "fmt",
            Self::Clippy => "clippy",
            Self::Browser => "browser",
            Self::Test => "test",
        }
    }

    /// The cargo commands the lint jobs run, in order, each as its arguments.
    const fn lints(self) -> &'static [&'static [&'static str]] {
        match self {
            Self::Fmt => &[&["fmt", "--all", "--", "--check"]],
            Self::Clippy => &[&["clippy", "--workspace", "--all-targets", "--all-features", "--", "-D", "warnings"]],
            Self::Browser => &[
                &[
                    "clippy",
                    "--workspace",
                    "--all-features",
                    "--target",
                    "wasm32-unknown-unknown",
                    "--",
                    "-D",
                    "warnings",
                ],
                // The browser chassis's examples are the pages themselves, which a native build compiles empty.
                &[
                    "clippy",
                    "-p",
                    "aethereal-chassis-browser",
                    "--all-targets",
                    "--target",
                    "wasm32-unknown-unknown",
                    "--",
                    "-D",
                    "warnings",
                ],
            ],
            Self::Test => &[],
        }
    }

    /// The cargo commands the job runs, in order, each as its arguments.
    fn commands(self, tests: &Tests) -> Result<Vec<Vec<String>>, Failure> {
        let commands = match (self, tests) {
            (Self::Test, Tests::Workspace) => test_commands(tests, &BTreeSet::new()),
            (Self::Test, Tests::Packages(_)) => test_commands(tests, &Workspace::load()?.libraries()),
            (Self::Fmt | Self::Clippy | Self::Browser, _) => {
                self.lints().iter().map(|arguments| command(arguments, Vec::new(), &[])).collect()
            }
        };
        Ok(commands)
    }
}

/// The test job's commands for `tests`, given the members with a library target.
///
/// nextest runs each test in a process of its own, and runs no doctests: the doctests, which hold the `compile_fail`
/// checks of what a declaration refuses, run after it under `cargo test`. nextest reads its profile from
/// `NEXTEST_PROFILE`, which CI sets to `ci`. `cargo test --doc` refuses a `-p` naming a package with no library, so the
/// doctests of a selection run only for its packages with one, and not at all when none has.
fn test_commands(tests: &Tests, libraries: &BTreeSet<String>) -> Vec<Vec<String>> {
    let (tested, documented) = match tests {
        Tests::Workspace => (vec![String::from("--workspace")], vec![String::from("--workspace")]),
        Tests::Packages(packages) => (package_args(packages.iter()), package_args(packages.intersection(libraries))),
    };

    let nextest = command(&["nextest", "run"], tested, &["--all-features"]);
    if documented.is_empty() {
        return vec![nextest];
    }
    vec![nextest, command(&["test"], documented, &["--all-features", "--doc"])]
}

/// `-p <package>` for each of `packages`.
fn package_args<'name>(packages: impl Iterator<Item = &'name String>) -> Vec<String> {
    packages.flat_map(|name| [String::from("-p"), name.clone()]).collect()
}

/// One command's arguments: `head`, the packages it runs for, and `tail`.
fn command(head: &[&str], packages: Vec<String>, tail: &[&str]) -> Vec<String> {
    head.iter().map(|arg| (*arg).to_owned()).chain(packages).chain(tail.iter().map(|arg| (*arg).to_owned())).collect()
}

/// Runs each job's commands, in order, stopping at the first that fails.
pub fn run(jobs: &[Job], tests: &Tests) -> Result<(), Failure> {
    for job in jobs {
        let name = job.name();
        eprintln!("running check={name}");
        for arguments in job.commands(tests)? {
            cargo::run(cargo::command().args(arguments), &format!("check={name}"))?;
        }
        eprintln!("passed check={name}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Tests, test_commands};
    use std::collections::BTreeSet;

    fn names(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_selection_s_doctests_name_only_its_packages_with_a_library() {
        // Catches `cargo test --doc -p xtask`, which cargo refuses as xtask has no library: a pull request that
        // changed xtask's own commands alone would fail its test job before a test ran.
        let libraries = names(&["aethereal-data", "aethereal-ui"]);

        let mixed = test_commands(&Tests::Packages(names(&["aethereal-ui", "xtask"])), &libraries);
        assert_eq!(mixed.len(), 2, "the tests, then the doctests");
        assert!(mixed[0].windows(2).any(|pair| pair == ["-p", "xtask"]), "xtask's tests run");
        assert!(!mixed[1].iter().any(|arg| arg == "xtask"), "xtask is never named for doctests");
        assert!(mixed[1].windows(2).any(|pair| pair == ["-p", "aethereal-ui"]), "the library's doctests run");

        let bin_only = test_commands(&Tests::Packages(names(&["xtask"])), &libraries);
        assert_eq!(bin_only.len(), 1, "no doctest command at all when no package selected has a library");
    }
}
