//! What CI runs, one list for CI and a local run: each of CI's jobs calls `cargo xtask check <job>`, and a bare
//! `cargo xtask check` runs every job, in order, stopping at the first that fails.

use crate::cargo;
use crate::failure::Failure;

/// One of CI's jobs, as the cargo commands it runs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Job {
    /// Format: the code is as rustfmt writes it.
    Fmt,
    /// Clippy: the workspace linted for the machine it is built on.
    Clippy,
    /// Browser host: the workspace linted for wasm32, where a browser's host itself runs (ADR-0001).
    Browser,
    /// Test: every test, then the doctests.
    Test,
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

    /// The cargo commands the job runs, in order, each as its arguments.
    const fn commands(self) -> &'static [&'static [&'static str]] {
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
            // nextest runs each test in a process of its own, and runs no doctests: the doctests, which hold the
            // `compile_fail` checks of what a declaration refuses, run after it under `cargo test`. nextest reads its
            // profile from `NEXTEST_PROFILE`, which CI sets to `ci`.
            Self::Test => &[
                &["nextest", "run", "--workspace", "--all-features"],
                &["test", "--workspace", "--all-features", "--doc"],
            ],
        }
    }
}

/// Runs each job's commands, in order, stopping at the first that fails.
pub fn run(jobs: &[Job]) -> Result<(), Failure> {
    for job in jobs {
        let name = job.name();
        eprintln!("running check={name}");
        for arguments in job.commands() {
            cargo::run(cargo::command().args(*arguments), &format!("check={name}"))?;
        }
        eprintln!("passed check={name}");
    }
    Ok(())
}
