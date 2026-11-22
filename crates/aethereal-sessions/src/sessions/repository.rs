//! Whether the hub's repository is served (ADR-0057, ADR-0059): what each session's container clones or fetches at
//! `git_url` as its start script bootstraps. `Sessions` depends on nothing that serves it: an actor of the hub's own,
//! which depends on both, hears where it stands from what serves it and tells `Sessions`, which then refuses a start the
//! repository cannot serve before any container is made, as that container's bootstrap would fail. Until such an actor
//! tells it anything, it does without: a program that runs none starts every session as it is asked.

use super::Sessions;
use super::asks::Refusal;
use aethereal_data::Plain;

/// Where the hub's repository stands, as the actor that hears it tells `Sessions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.repository")]
pub enum Repository {
    /// It is being made: no session can start until it is served.
    Making,
    /// It is served.
    Ready,
    /// It could not be made, or its hook is not in place, so it is never served.
    Unmade,
}

/// What `Sessions` knows of the hub's repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub(super) enum Served {
    /// Nothing has told it where the repository stands: a start goes ahead.
    Unheard,
    /// The actor that hears it said it stands so.
    Told(Repository),
}

impl Sessions {
    /// Where the hub's repository stands, as the actor that hears it says.
    pub(super) const fn handle_repository(&mut self, repository: Repository) {
        self.served = Served::Told(repository);
    }

    /// Whether a session may start as the hub's repository stands, or else why not: it may when the settings name no
    /// repository, when nothing has said where it stands, or when it is served.
    pub(super) fn may_serve(&self) -> Result<(), Refusal> {
        if self.settings.git_url.is_empty() {
            return Ok(());
        }

        match self.served {
            Served::Unheard | Served::Told(Repository::Ready) => Ok(()),
            Served::Told(Repository::Making) => Err(Refusal::RepositoryMaking),
            Served::Told(Repository::Unmade) => Err(Refusal::RepositoryUnmade),
        }
    }
}
