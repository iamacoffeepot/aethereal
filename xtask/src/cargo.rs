//! Cargo, run from the workspace's root, and the other tools a command runs.

use crate::failure::Failure;
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
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

/// Runs `command`, cargo or another tool, its output the terminal's. One that does not succeed fails, printed as
/// `failed <what>`, where `what` is a `key=value` naming the step: the tool has printed why above it.
pub fn run(command: &mut Command, what: &str) -> Result<(), Failure> {
    let succeeded = command.status()?.success();
    if succeeded {
        Ok(())
    } else {
        Err(Failure::new(format_args!("failed {what}")))
    }
}

/// Where cargo writes what it builds, as `cargo metadata` reports it.
pub fn target_directory() -> Result<PathBuf, Failure> {
    let output = command().args(["metadata", "--format-version", "1", "--no-deps"]).output()?;
    let failed = !output.status.success();
    if failed {
        return Err(Failure::new(String::from_utf8_lossy(&output.stderr)));
    }

    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| Failure::new(format_args!("cargo metadata: {error}")))?;
    metadata["target_directory"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| Failure::new("cargo metadata named no target_directory"))
}
