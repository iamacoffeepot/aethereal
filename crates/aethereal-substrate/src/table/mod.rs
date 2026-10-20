//! The engine's one table of actors (ADR-0008).

mod actors;
#[cfg(test)]
mod tests;

use crate::by_actor::IdBits;
use crate::id::{child_id, path_id};
use crate::{Column, Context, Folded, Routed};
pub use actors::Actors;
use actors::Entry;
use aethereal_actor::{BlobHash, BlobId, Declared, Effect, SeatNumber, Seated, Table, Time};
use aethereal_data::{ActorId, ActorPath, PathSegment};
use std::collections::HashSet;
use std::hash::BuildHasherDefault;
use std::num::NonZeroUsize;

/// The table as a turn reads it. The id of a child is computed here, by the engine, and nowhere else.
impl Table for ActorTable {
    fn resolve(&self, path: &ActorPath) -> Option<Seated> {
        let id = Self::resolve(self, path)?;

        Some(Seated { id, seat: Self::seat(self, id)? })
    }

    fn child(&self, parent: ActorId, segment: &PathSegment) -> ActorId {
        child_id(parent, segment)
    }

    fn is_live(&self, id: ActorId) -> bool {
        Self::is_live(self, id)
    }

    fn held(&self, id: ActorId) -> bool {
        Self::is_live(self, id)
    }

    fn child_seat(&self, id: ActorId) -> Option<SeatNumber> {
        Self::seat(self, id)
    }

    fn declared(&self, id: ActorId) -> Option<&'static Declared> {
        self.entry(id).map(Entry::declared)
    }

    /// The table holds no blobs: a turn reads them through what the engine opens it with, beside the table.
    fn blob(&self, _: BlobId) -> Option<&[u8]> {
        None
    }

    fn blob_hash(&self, _: BlobId) -> Option<BlobHash> {
        None
    }

    /// The table holds no time: a turn reads its step's through what the engine opens it with, beside the table.
    fn now(&self) -> Time {
        Time::ZERO
    }
}

/// The engine's one table of live actors: which there are, and where each stands (ADR-0008).
///
/// It is a view (ADR-0011), folded from the spawns and endings of finished
/// turns. A turn reads it as it stood when the turn's step began, and it
/// changes only between two steps.
///
/// It holds the live actors. An actor is written once, when it is made, and
/// taken out once, by its own ending, so for whoever holds a reference an
/// actor that is not here has ended. The ending of an actor does not end the
/// actors beneath it (ADR-0009). Whether a key is free is the parent's own
/// knowledge, kept in its seat; the table keeps the roots the chassis has
/// made, for the engine's life.
pub struct ActorTable {
    actors: Folded<Actors>,
    /// The ids of the roots made, live or ended.
    roots: HashSet<u64, BuildHasherDefault<IdBits>>,
}

impl ActorTable {
    /// The empty table, its actors spread over `parts` parts.
    #[must_use]
    pub fn new(parts: NonZeroUsize) -> Self {
        Self { actors: Folded::new(parts), roots: HashSet::default() }
    }

    /// Folds in the effects of one turn, emitted in `context`, in the order given.
    ///
    /// # Panics
    ///
    /// Panics when an effect makes an actor under an id the table already holds (ADR-0008).
    pub fn fold(&mut self, effects: &[Effect], context: &Context) {
        // The table keeps nothing a record names in an arena.
        self.actors.fold(effects, context, &[]);
    }

    /// Notes that the chassis has made the root `id`, whose key is then never used again.
    pub(crate) fn made_root(&mut self, id: ActorId) {
        self.roots.insert(id.bits());
    }

    /// An empty row of buckets for one writer's records of the table.
    pub(crate) fn routed(&self) -> Routed<Actors> {
        self.actors.routed()
    }

    /// Fills `column` with the part numbered `part` and each row's bucket for it, as [`Folded::fill`] does. Folding
    /// the column panics when a record makes an actor under an id the table already holds (ADR-0008).
    pub(crate) fn fill<'r>(
        &mut self,
        part: usize,
        rows: impl IntoIterator<Item = &'r mut Routed<Actors>>,
        column: &mut Column<Actors>,
    ) {
        self.actors.fill(part, rows, column);
    }

    /// Folds each row's records for the part numbered `part` where it lies, as [`Folded::fold_part`] does. It panics
    /// when a record makes an actor under an id the table already holds (ADR-0008).
    pub(crate) fn fold_part<'r>(&mut self, part: usize, rows: impl IntoIterator<Item = &'r mut Routed<Actors>>) {
        self.actors.fold_part(part, rows, &[]);
    }

    /// Empties `column` back into the part numbered `part`, as [`Folded::empty`] does.
    pub(crate) fn empty<'r>(
        &mut self,
        part: usize,
        column: &mut Column<Actors>,
        rows: impl IntoIterator<Item = &'r mut Routed<Actors>>,
    ) {
        self.actors.empty(part, column, rows);
    }

    /// Whether the actor `id` stands for is live: made, and not ended.
    #[must_use]
    pub fn is_live(&self, id: ActorId) -> bool {
        self.entry(id).is_some()
    }

    /// Whether a root has been made under the id `id`, live or ended.
    #[must_use]
    pub fn holds_root(&self, id: ActorId) -> bool {
        self.roots.contains(&id.bits())
    }

    /// The id of the actor `path` names, when that actor is live.
    ///
    /// Its entry is the one at the path's id, with the path's last segment, beneath the actor at the id of the
    /// path's parent: the actors above need not be live.
    #[must_use]
    pub fn resolve(&self, path: &ActorPath) -> Option<ActorId> {
        let id = path_id(path);
        let entry = self.entry(id)?;
        let is_last = *entry.segment() == path.last();
        let is_beneath = entry.above() == path.parent().as_ref().map(path_id);
        let resolves = is_last && is_beneath;

        resolves.then_some(id)
    }

    /// The path `id` stands for, when its actor and every actor above it are live.
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

    /// The seat of the actor `id` stands for, when it is live.
    #[must_use]
    pub fn seat(&self, id: ActorId) -> Option<SeatNumber> {
        self.entry(id).map(Entry::seat)
    }

    /// The actor directly above the one `id` stands for, when that one is live and is not a root.
    #[must_use]
    pub fn parent(&self, id: ActorId) -> Option<ActorId> {
        self.entry(id)?.above()
    }

    /// The entry of the actor `id` stands for, when it is in the table.
    fn entry(&self, id: ActorId) -> Option<&Entry> {
        self.actors.shard(id.bits()).get(&id.bits())
    }
}
