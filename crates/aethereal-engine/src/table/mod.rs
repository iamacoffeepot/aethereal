//! The engine's one table of actors (ADR-0008).

mod change;
mod entry;
mod node;
mod refused;
#[cfg(test)]
mod tests;

pub use change::Change;
pub use entry::Standing;
pub use refused::{NotLive, Refused};

use crate::id::{child_id, path_id, root_id};
use aethereal_data::{ActorId, ActorPath, PathSegment, PathTooDeep};
use change::Changes;
use entry::Entry;
use node::Node;

/// The engine's one table of actors: which there are, where each stands, and which have closed (ADR-0008).
///
/// The table only grows. An actor's line is written once, when it goes live,
/// and marked once, when it closes, and each of the two carries the number of
/// the change that made it. A reader takes [`Self::latest`] and passes it to
/// what it reads, and sees the table as it stood at that change.
///
/// There is one table for the whole engine, shared between its threads. A
/// reader never waits on a writer.
#[derive(Default)]
pub struct ActorTable {
    /// Every line, live or closed, found by its id.
    lines: Node,
    /// The count the changes are numbered from.
    changes: Changes,
}

impl ActorTable {
    /// The empty table, at change zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The latest change: what a reader takes before it reads.
    #[must_use]
    pub fn latest(&self) -> Change {
        self.changes.latest()
    }

    /// Makes the root `segment` names live. Only the chassis makes a root.
    pub fn add_root(&self, segment: &PathSegment) -> Result<ActorId, Refused> {
        self.insert(root_id(segment), ActorPath::from_root(segment))
    }

    /// Makes the child `segment` names live beneath the live actor `parent`.
    pub fn add_child(&self, parent: ActorId, segment: &PathSegment) -> Result<ActorId, Refused> {
        let above = self.live(parent).ok_or(Refused::ParentNotLive)?;
        let path = above.path().child(segment).map_err(|PathTooDeep| Refused::TooDeep)?;

        self.insert(child_id(parent, segment), path)
    }

    /// Marks the actor `id` stands for closed, and answers with the change that closed it.
    pub fn close(&self, id: ActorId) -> Result<Change, NotLive> {
        let entry = self.live(id).ok_or(NotLive)?;

        self.changes.make(|change| {
            if entry.close(change) {
                Ok(change)
            } else {
                Err(NotLive)
            }
        })
    }

    /// How the actor `id` stands for stood at the change `at`.
    #[must_use]
    pub fn standing(&self, id: ActorId, at: Change) -> Standing {
        self.lines.find(id).map_or(Standing::Absent, |entry| entry.standing(at))
    }

    /// The id of the actor `path` names, when that actor was live at the change `at`.
    #[must_use]
    pub fn resolve(&self, path: &ActorPath, at: Change) -> Option<ActorId> {
        let entry = self.lines.find(path_id(path))?;
        let is_named = entry.names(path);
        let is_live = entry.standing(at) == Standing::Live;
        let resolves = is_named && is_live;

        resolves.then_some(entry.id())
    }

    /// The path `id` stands for, whether its actor is live or closed.
    #[must_use]
    pub fn path(&self, id: ActorId) -> Option<ActorPath> {
        self.lines.find(id).map(|entry| entry.path().clone())
    }

    /// The line of `id`, when its actor has not closed.
    fn live(&self, id: ActorId) -> Option<&Entry> {
        self.lines.find(id).filter(|entry| entry.is_live())
    }

    /// Writes the line of the actor at `path` under `id`, as one numbered change.
    ///
    /// An id already in the table is refused before a number is taken, so a
    /// refusal is not a change. Only a writer that loses its id to another
    /// between that look and its write is refused holding a number.
    fn insert(&self, id: ActorId, path: ActorPath) -> Result<ActorId, Refused> {
        match self.lines.find(id) {
            Some(held) => Err(held.refusal(&path)),
            None => self.changes.make(|change| self.lines.place(Entry::new(id, path, change))).map(|()| id),
        }
    }
}
