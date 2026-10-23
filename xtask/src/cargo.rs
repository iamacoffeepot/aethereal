//! Cargo, run from the workspace's root.

use crate::failure::Failure;
use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

/// The workspace's root, the directory above this crate's.
pub fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

/// A cargo command run from the workspace's root: the cargo that ran this tool, which `cargo run` names in `CARGO`.
pub fn command() -> Command {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let mut command = Command::new(cargo);
    command.current_dir(root());
    command
}

/// Runs `command`, its output the terminal's. One that does not succeed fails, printed as `failed <what>`, where
/// `what` is a `key=value` naming the step: cargo has printed why above it.
pub fn run(command: &mut Command, what: &str) -> Result<(), Failure> {
    let succeeded = command.status()?.success();
    if succeeded {
        Ok(())
    } else {
        Err(Failure::new(format_args!("failed {what}")))
    }
}
