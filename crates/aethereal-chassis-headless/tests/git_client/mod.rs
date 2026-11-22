//! git's own client, as the tests that reach the hub's repository run it: a program of this machine, reading none of
//! its configuration. What the git tests and the sessions' share.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Whether this machine has git's client to run: a test that needs it passes, saying so, where it has none.
#[allow(clippy::print_stderr)] // A test skipped says so, for whoever runs it.
pub fn has_git() -> bool {
    let ran = Command::new("git").arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status();
    let has = ran.is_ok_and(|status| status.success());
    if !has {
        eprintln!("skipped: this machine has no `git` to run");
    }

    has
}

/// git's client, run in `dir` with `args`: reading no configuration of this machine's or its user's, prompting for
/// nothing, and committing as the test.
pub fn git(dir: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid");

    command
}

/// The session image's directory, beside the sessions' crate: the start script, the credential helper, and the system
/// config that names it.
pub fn image() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../aethereal-sessions/image")
}

/// The path a container's programs are found on: the image's directory, where git finds the credential helper, before
/// this machine's own.
pub fn searched() -> String {
    let machine = env::var("PATH").unwrap_or_default();

    format!("{}:{machine}", image().display())
}
