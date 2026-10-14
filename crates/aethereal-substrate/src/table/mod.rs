//! The engine's one table of actors (ADR-0008).

mod actors;
#[cfg(test)]
mod tests;

use crate::Folded;
use crate::id::{child_id, path_id};
use actors::{Actors, Entry};
use aethereal_actor::{Effect, Made, Table};
use aethereal_data::{ActorId, ActorPath, PathSegment};
use std::num::NonZeroUsize;

/// The table proves a reference: an id names an actor once the table holds it, live or ended (ADR-0008).
impl Made for ActorTable {
    fn made(&self, id: ActorId) -> bool {
        self.standing(id) != Standing::Absent
    }
}

/// The table as a turn reads it. The id of a child is computed here, by the engine, and nowhere else.
impl Table for ActorTable {
    fn resolve(&self, path: &ActorPath) -> Option<ActorId> {
        Self::resolve(self, path)
    }

    fn path(&self, id: ActorId) -> Option<ActorPath> {
        Self::path(self, id)
    }

    fn child(&self, parent: ActorId, segment: &PathSegment) -> ActorId {
        child_id(parent, segment)
    }
}

/// How an actor stands in the table (ADR-0008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// Not in the table: never made.
    Absent,
    /// Made, and not ended.
    Live,
    /// Ended. Its path is never used again.
    Ended,
}

/// The engine's one table of actors: which there are, where each stands, and which have ended (ADR-0008).
///
/// It is a view (ADR-0011), folded from the spawns and endings of finished
/// turns. A turn reads it as it stood when the turn's step began, and it
/// changes only between two steps.
///
/// The table only grows. An actor is written once, when it is made, and
/// marked once, by its own ending. The ending of an actor does not end the
/// actors beneath it (ADR-0009).
pub struct ActorTable {
    actors: Folded<Actors>,
}

impl ActorTable {
    /// The empty table, its actors spread over `parts` parts.
    #[must_use]
    pub fn new(parts: NonZeroUsize) -> Self {
        Self { actors: Folded::new(parts) }
    }

    /// Folds in the effects of one step's turns, in the order given.
    ///
    /// # Panics
    ///
    /// Panics when an effect makes an actor under an id the table already holds (ADR-0008).
    pub fn fold(&mut self, effects: &[Effect]) {
        self.actors.fold(effects, ());
    }

    /// How the actor `id` stands for stands.
    #[must_use]
    pub fn standing(&self, id: ActorId) -> Standing {
        match self.entry(id) {
            None => Standing::Absent,
            Some(entry) if entry.is_live() => Standing::Live,
            Some(_) => Standing::Ended,
        }
    }

    /// The id of the actor `path` names, when that actor is live.
    #[must_use]
    pub fn resolve(&self, path: &ActorPath) -> Option<ActorId> {
        let id = path_id(path);
        let is_live = self.standing(id) == Standing::Live;
        let is_named = self.path(id).as_ref() == Some(path);
        let resolves = is_live && is_named;

        resolves.then_some(id)
    }

    /// The path `id` stands for, whether its actor is live or ended.
    #[must_use]
    pub fn path(&self, id: ActorId) -> Option<ActorPath> {
        let mut entry = self.entry(id)?;
        let mut segments = vec![entry.segment()];
        while let Some(above) = entry.above() {
            let is_too_deep = segments.len() >= ActorPath::MAX_DEPTH;
            if is_too_deep {
                return None;
            }
            entry = self.entry(above)?;
            segments.push(entry.segment());
        }

        let root = ActorPath::from_root(segments.pop()?);

        segments.iter().rev().try_fold(root, |path, segment| path.child(segment)).ok()
    }

    /// The actor directly above the one `id` stands for, when that one is in the table and is not a root.
    #[must_use]
    pub fn parent(&self, id: ActorId) -> Option<ActorId> {
        self.entry(id)?.above()
    }

    /// The entry of the actor `id` stands for, when it is in the table.
    fn entry(&self, id: ActorId) -> Option<&Entry> {
        self.actors.shard(id.bits()).get(&id.bits())
    }
}
