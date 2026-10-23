//! The workspace's commands, run as `cargo xtask` through the alias in `.cargo/config.toml`:
//!
//! ```text
//! cargo xtask example <example> [--serve [--port <port>]]   one browser example's page, built and served
//! cargo xtask check [fmt | clippy | browser | test]...      what CI runs; every check when none is named
//! ```
//!
//! **A native-only tool** (ADR-0001): it runs cargo and serves files on the machine it is run on. A build for `wasm32`
//! compiles it empty. It is no default member of the workspace, so a plain `cargo build` or `cargo test` never compiles
//! it.
#![allow(clippy::print_stdout, clippy::print_stderr)] // A command prints what it did, for a person or a script to read.

#[cfg(not(target_arch = "wasm32"))]
mod args;
#[cfg(not(target_arch = "wasm32"))]
mod cargo;
#[cfg(not(target_arch = "wasm32"))]
mod check;
#[cfg(not(target_arch = "wasm32"))]
mod example;
#[cfg(not(target_arch = "wasm32"))]
mod failure;

#[cfg(not(target_arch = "wasm32"))]
use std::env;
#[cfg(not(target_arch = "wasm32"))]
use std::process::ExitCode;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> ExitCode {
    let ran = args::parse(env::args().skip(1)).and_then(|command| match command {
        args::Command::Example { name, serving } => example::run(&name, serving),
        args::Command::Check(jobs) => check::run(&jobs),
    });
    match ran {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => failure.report(),
    }
}

#[cfg(target_arch = "wasm32")]
const fn main() {}
