//! The git capability's settings (ADR-0028, ADR-0057).

use aethereal_data::Plain;

/// The git capability's section, `git`: the repository it serves, the program it serves it with, the hooks every push
/// runs, and how many programs run at once (ADR-0057). They are the root's config.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.git.settings")]
#[settings]
pub struct GitSettings {
    /// The bare repository served, a path on the host's machine: made with `git init --bare` as the root begins, which
    /// leaves one already there as it is. A relative path is the hub's working directory's.
    pub repository: String,
    /// The git program, a path on the host's machine, or a name found on the `PATH` it is given.
    pub program: String,
    /// The directory of the hooks every push runs, git's `core.hooksPath`, which holds the crate's `pre-receive`: an
    /// absolute path, as git reads a relative one from the repository. By default the crate's own `hooks`, where it
    /// was built.
    pub hooks: String,
    /// The most `git http-backend` programs run at once, one for each request served: past it, a request is answered
    /// `503`.
    pub most_running: u32,
}

impl Default for GitSettings {
    fn default() -> Self {
        Self {
            repository: String::from("git/repository.git"),
            program: String::from("git"),
            hooks: String::from(concat!(env!("CARGO_MANIFEST_DIR"), "/hooks")),
            most_running: 8,
        }
    }
}
