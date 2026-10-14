//! The view of which actors there are (ADR-0008, ADR-0011).

use crate::id::spawned;
use crate::{ByActor, Context, View};
use aethereal_actor::{Effect, SeatNumber, What};
use aethereal_data::{ActorId, PathSegment};

/// The view of which actors there are, keyed by an actor's own id (ADR-0011).
///
/// A key has one source for each of its two changes. An actor's entry is made
/// by the spawn its parent asked for, or for a root by the chassis. It is
/// marked ended by the actor's own ending and by nothing else: no ending of
/// another actor, its parent's included, reaches it (ADR-0009).
pub struct Actors;

/// One actor's entry: where it stands, and whether it has ended.
pub struct Entry {
    /// The actor directly above. None for a root, which stands beneath nothing: fixed when the entry is made.
    above: Option<ActorId>,
    /// The actor's own segment of its path.
    segment: PathSegment,
    /// The seat the actor sits in, or sat in once it has gone.
    seat: SeatNumber,
    /// Whether the actor has ended.
    life: Life,
}

/// Whether an actor in the table has ended.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Life {
    Live,
    Ended,
}

/// What the view keeps of an effect.
pub enum Record {
    /// An actor was made at `segment`, beneath `above` where it is not a root, in `seat`.
    Made { above: Option<ActorId>, segment: PathSegment, seat: SeatNumber },
    /// The actor ended.
    Ended,
}

impl Entry {
    /// The actor directly above, or none for a root.
    pub(super) const fn above(&self) -> Option<ActorId> {
        self.above
    }

    /// The actor's own segment of its path.
    pub(super) const fn segment(&self) -> &PathSegment {
        &self.segment
    }

    /// The seat the actor sits in, or sat in.
    pub(super) const fn seat(&self) -> SeatNumber {
        self.seat
    }

    /// Whether the actor has not ended.
    pub(super) fn is_live(&self) -> bool {
        self.life == Life::Live
    }
}

impl View for Actors {
    type Shard = ByActor<Entry>;
    type Record = Record;

    fn takes<'a>(effect: &'a Effect, _: &'a Context) -> impl Iterator<Item = (u64, Record)> + 'a {
        record(effect).into_iter()
    }

    /// # Panics
    ///
    /// Panics when an actor is made under an id the table already holds (ADR-0008). A parent refuses a key it has
    /// used before it asks, so that is two paths with one id or a fault in the engine, and neither has a way on.
    fn fold(shard: &mut ByActor<Entry>, key: u64, record: Record) {
        match record {
            Record::Made { above, segment, seat } => {
                let held = shard.insert(key, Entry { above, segment, seat, life: Life::Live });
                assert!(
                    held.is_none(),
                    "actor table: an actor was made under the id {key:#018x}, which is already held"
                );
            }
            Record::Ended => {
                if let Some(entry) = shard.get_mut(&key) {
                    entry.life = Life::Ended;
                }
            }
        }
    }
}

/// The key and record the view keeps of `effect`, or none for an effect it does not fold.
fn record(effect: &Effect) -> Option<(u64, Record)> {
    let by = effect.by();
    let seat = effect.seated().seat;
    let (id, record) = match effect.what() {
        What::Root { from } => (by, Record::Made { above: None, segment: from.singleton(), seat }),
        What::RootInstanced { from, key } => (by, Record::Made { above: None, segment: from.instanced(key), seat }),
        What::Failed(_) | What::Close => (by, Record::Ended),
        _ => {
            let (child, segment) = spawned(effect)?;

            (child.id, Record::Made { above: Some(by), segment, seat: child.seat })
        }
    };

    Some((id.bits(), record))
}
