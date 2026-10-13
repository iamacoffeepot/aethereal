//! One actor's place in the tree of actors (ADR-0008).

use super::radix::Radix;
use super::{Change, Refused};
use aethereal_data::{ActorId, ActorPath};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

/// What an actor's closing number holds while the actor is live: later than every change.
const LIVE: u64 = u64::MAX;

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

/// One actor's place in the tree: written once when the actor goes live, and marked once when it closes.
///
/// Its parent's children hold it, and it holds its own, so the tree is the
/// actors' own lineage. Only the turn of the actor itself adds a child beneath
/// it, so its children have one writer.
pub(super) struct Actor {
    /// The id the path folds to.
    id: ActorId,
    /// Where the actor stands.
    path: ActorPath,
    /// The actor above. A root's points at nothing.
    parent: Weak<Self>,
    /// The change that made the actor live.
    live: Change,
    /// The number of the change that closed the actor, and [`LIVE`] until one does. It is an atomic because the
    /// thread that closes the actor is not the ones reading it: any turn may ask how the actor stands.
    closed: AtomicU64,
    /// The actors directly beneath, each under its segment's hash.
    children: Radix,
}

impl Actor {
    /// The actor at `path` beneath `parent`, made live by `live`.
    pub(super) fn new(id: ActorId, path: ActorPath, parent: Weak<Self>, live: Change) -> Self {
        Self { id, path, parent, live, closed: AtomicU64::new(LIVE), children: Radix::default() }
    }

    /// The actor's id.
    pub(super) const fn id(&self) -> ActorId {
        self.id
    }

    /// Where the actor stands.
    pub(super) const fn path(&self) -> &ActorPath {
        &self.path
    }

    /// The actor above, when this one is not a root.
    pub(super) fn parent(&self) -> Option<Arc<Self>> {
        self.parent.upgrade()
    }

    /// The actors directly beneath.
    pub(super) const fn children(&self) -> &Radix {
        &self.children
    }

    /// Whether the actor has not closed, as of every change made so far.
    pub(super) fn is_live(&self) -> bool {
        self.closed.load(Ordering::Acquire) == LIVE
    }

    /// Marks the actor closed by `change`. False when it was closed already, and the first mark stands.
    pub(super) fn close(&self, change: Change) -> bool {
        self.closed.compare_exchange(LIVE, change.0, Ordering::AcqRel, Ordering::Relaxed).is_ok()
    }

    /// How the actor stands to a reader holding `at`, who passes over anything numbered later.
    pub(super) fn standing(&self, at: Change) -> Standing {
        let is_made = self.live <= at;
        let is_closed = self.closed.load(Ordering::Acquire) <= at.0;

        match (is_made, is_closed) {
            (false, _) => Standing::Absent,
            (true, false) => Standing::Live,
            (true, true) => Standing::Closed,
        }
    }

    /// Why a new actor at `path`, whose id is this one's, is refused.
    pub(super) fn refusal(&self, path: &ActorPath) -> Refused {
        if self.path == *path {
            Refused::KeyTaken
        } else {
            Refused::IdShared
        }
    }
}
