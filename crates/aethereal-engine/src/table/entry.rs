//! One actor's line in the table (ADR-0008).

use super::{Change, Refused};
use aethereal_data::{ActorId, ActorPath};
use std::sync::OnceLock;

/// How an actor stands in the table, to a reader holding one change's number (ADR-0008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// Not in the table at that change: never made, or made later.
    Absent,
    /// Made at or before that change, and not closed by it.
    Live,
    /// Closed at or before that change. Its path is never used again.
    Closed,
}

/// One actor's line: written once when the actor goes live, and marked once when it closes.
pub(super) struct Entry {
    /// The id the path folds to.
    id: ActorId,
    /// Where the actor stands.
    path: ActorPath,
    /// The change that made the actor live.
    live: Change,
    /// The change that closed the actor; empty while it is live. It is written once and never cleared, by a
    /// thread other than the ones reading it, which is why it is a `OnceLock` and not a field a turn owns.
    closed: OnceLock<Change>,
}

impl Entry {
    /// The line of the actor at `path`, made live by `live`.
    pub(super) const fn new(id: ActorId, path: ActorPath, live: Change) -> Self {
        Self { id, path, live, closed: OnceLock::new() }
    }

    /// The actor's id.
    pub(super) const fn id(&self) -> ActorId {
        self.id
    }

    /// Where the actor stands.
    pub(super) const fn path(&self) -> &ActorPath {
        &self.path
    }

    /// Whether this is the line of the actor `path` names.
    pub(super) fn names(&self, path: &ActorPath) -> bool {
        self.path == *path
    }

    /// Whether the actor has not closed, as of every change made so far.
    pub(super) fn is_live(&self) -> bool {
        self.closed.get().is_none()
    }

    /// Marks the actor closed by `change`. False when it was closed already, and the first mark stands.
    pub(super) fn close(&self, change: Change) -> bool {
        self.closed.set(change).is_ok()
    }

    /// How the actor stands to a reader holding `at`, who passes over anything numbered later.
    pub(super) fn standing(&self, at: Change) -> Standing {
        let is_made = self.live <= at;
        let is_closed = self.closed.get().is_some_and(|closed| *closed <= at);

        match (is_made, is_closed) {
            (false, _) => Standing::Absent,
            (true, false) => Standing::Live,
            (true, true) => Standing::Closed,
        }
    }

    /// Why a new actor at `path`, whose id is this line's, is refused.
    pub(super) fn refusal(&self, path: &ActorPath) -> Refused {
        if self.names(path) {
            Refused::KeyTaken
        } else {
            Refused::IdShared
        }
    }
}
