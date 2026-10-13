//! The engine's one table of actors (ADR-0008).

mod actor;
mod change;
mod radix;
mod refused;
#[cfg(test)]
mod tests;

pub use actor::Standing;
pub use change::Change;
pub use refused::{NotLive, Refused};

use crate::id::{child_id, path_id, root_id, segment_hash};
use actor::Actor;
use aethereal_data::{ActorId, ActorPath, PathSegment, PathTooDeep};
use change::Changes;
use radix::Radix;
use std::sync::{Arc, Weak};

/// The engine's one table of actors: which there are, where each stands, and which have closed (ADR-0008).
///
/// The table is the tree of actors itself: each actor holds the actors
/// directly beneath it, and only its own turn adds one there. Beside the tree
/// is an index from an id to its actor, for a reader that holds only the id.
///
/// The table only grows. An actor is written once, when it goes live, and
/// marked once, when it closes, and each of the two carries the number of the
/// change that made it. A reader takes [`Self::latest`] and passes it to what
/// it reads, and sees the table as it stood at that change.
///
/// There is one table for the whole engine, shared between its threads. A
/// reader never waits on a writer.
#[derive(Default)]
pub struct ActorTable {
    /// The roots, each under its segment's hash. The chassis alone adds one.
    roots: Radix,
    /// Every actor, live or closed, under its id. It points into the tree and owns nothing of it.
    index: Radix,
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
        self.insert(&self.roots, Weak::new(), root_id(segment), segment_hash(segment), ActorPath::from_root(segment))
    }

    /// Makes the child `segment` names live beneath the live actor `parent`.
    pub fn add_child(&self, parent: ActorId, segment: &PathSegment) -> Result<ActorId, Refused> {
        let above = self.live(parent).ok_or(Refused::ParentNotLive)?;
        let path = above.path().child(segment).map_err(|PathTooDeep| Refused::TooDeep)?;

        self.insert(above.children(), Arc::downgrade(above), child_id(parent, segment), segment_hash(segment), path)
    }

    /// Marks the actor `id` stands for closed, and answers with the change that closed it.
    pub fn close(&self, id: ActorId) -> Result<Change, NotLive> {
        let actor = self.live(id).ok_or(NotLive)?;

        self.changes.make(|change| {
            if actor.close(change) {
                Ok(change)
            } else {
                Err(NotLive)
            }
        })
    }

    /// How the actor `id` stands for stood at the change `at`.
    #[must_use]
    pub fn standing(&self, id: ActorId, at: Change) -> Standing {
        self.index.find(id.bits()).map_or(Standing::Absent, |actor| actor.standing(at))
    }

    /// The id of the actor `path` names, when that actor was live at the change `at`.
    #[must_use]
    pub fn resolve(&self, path: &ActorPath, at: Change) -> Option<ActorId> {
        let actor = self.index.find(path_id(path).bits())?;
        let is_named = actor.path() == path;
        let is_live = actor.standing(at) == Standing::Live;
        let resolves = is_named && is_live;

        resolves.then_some(actor.id())
    }

    /// The path `id` stands for, whether its actor is live or closed.
    #[must_use]
    pub fn path(&self, id: ActorId) -> Option<ActorPath> {
        self.index.find(id.bits()).map(|actor| actor.path().clone())
    }

    /// The actor directly above the one `id` stands for, when that one is in the table and is not a root.
    #[must_use]
    pub fn parent(&self, id: ActorId) -> Option<ActorId> {
        self.index.find(id.bits())?.parent().map(|above| above.id())
    }

    /// The actors directly beneath the one `id` stands for that were live at the change `at`, in no stated order.
    #[must_use]
    pub fn children(&self, id: ActorId, at: Change) -> Vec<ActorId> {
        let beneath = self.index.find(id.bits()).map(|actor| actor.children().all()).unwrap_or_default();

        beneath.iter().filter(|child| child.standing(at) == Standing::Live).map(|child| child.id()).collect()
    }

    /// The actor `id` stands for, when it has not closed.
    fn live(&self, id: ActorId) -> Option<&Arc<Actor>> {
        self.index.find(id.bits()).filter(|actor| actor.is_live())
    }

    /// Writes the actor at `path` under `id` among `siblings`, beneath `parent`, as one numbered change.
    ///
    /// An id already in the table is refused before a number is taken, so a
    /// refusal is not a change. Only a writer that loses its id to another
    /// between that look and its write is refused holding a number. The index
    /// is written first, since it is where two branches can meet; `siblings`
    /// has one writer, and two segments with one hash beneath one parent fold
    /// to one id, which the index has refused by then.
    fn insert(
        &self,
        siblings: &Radix,
        parent: Weak<Actor>,
        id: ActorId,
        segment: u64,
        path: ActorPath,
    ) -> Result<ActorId, Refused> {
        if let Some(held) = self.index.find(id.bits()) {
            return Err(held.refusal(&path));
        }

        self.changes.make(|change| {
            let actor = Arc::new(Actor::new(id, path, parent, change));
            self.index.place(id.bits(), &actor).map_err(|held| held.refusal(actor.path()))?;
            siblings.place(segment, &actor).map_err(|held| held.refusal(actor.path()))?;

            Ok(id)
        })
    }
}
