//! The view of which actors there are (ADR-0008, ADR-0011).

use crate::by_actor::IdBits;
use crate::id::spawned;
use crate::{ByActor, Context, View};
use aethereal_actor::{Declared, Effect, SeatNumber, What};
use aethereal_data::{ActorId, PathSegment};
use std::collections::HashSet;
use std::hash::BuildHasherDefault;

/// The view of which actors are live, keyed by an actor's own id (ADR-0011).
///
/// A key has one source for each of its two changes. An actor's entry is made
/// by the spawn its parent asked for, or for a root by the chassis. It is
/// taken out by the actor's own ending and by nothing else: no ending of
/// another actor, its parent's included, reaches it (ADR-0009). Beside that,
/// the actor's spawns add to its entry the children it has made.
pub struct Actors;

/// One live actor's entry: where it stands, and the children it has made.
pub struct Entry {
    /// The actor directly above. None for a root, which stands beneath nothing: fixed when the entry is made.
    above: Option<ActorId>,
    /// The actor's own segment of its path.
    segment: PathSegment,
    /// The seat the actor sits in, or sat in once it has gone.
    seat: SeatNumber,
    /// What the actor's type declares (ADR-0013).
    declared: &'static Declared,
    /// The ids of the children it has made, live or ended, whose keys it may not use again (ADR-0008).
    made: HashSet<u64, BuildHasherDefault<IdBits>>,
}

/// What the view keeps of an effect.
pub enum Record {
    /// An actor of the type `declared` describes was made at `segment`, beneath `above` where it is not a root, in
    /// `seat`.
    Made { above: Option<ActorId>, segment: PathSegment, seat: SeatNumber, declared: &'static Declared },
    /// The actor made the child with this id.
    MadeChild(u64),
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

    /// What the actor's type declares.
    pub(super) const fn declared(&self) -> &'static Declared {
        self.declared
    }

    /// Whether the actor has made the child with the id `child`.
    pub(super) fn has_made(&self, child: ActorId) -> bool {
        self.made.contains(&child.bits())
    }
}

impl View for Actors {
    type Shard = ByActor<Entry>;
    type Record = Record;

    fn takes<'a>(effect: &'a Effect, _: &'a Context) -> impl Iterator<Item = (u64, Record)> + 'a {
        let (record, made) = records(effect);

        record.into_iter().chain(made)
    }

    /// # Panics
    ///
    /// Panics when an actor is made under an id the table already holds (ADR-0008). A parent refuses a key it has
    /// used before it asks, so that is two paths with one id or a fault in the engine, and neither has a way on.
    fn fold(shard: &mut ByActor<Entry>, key: u64, record: Record) {
        match record {
            Record::Made { above, segment, seat, declared } => {
                let made = HashSet::default();
                let held = shard.insert(key, Entry { above, segment, seat, declared, made });
                assert!(
                    held.is_none(),
                    "actor table: an actor was made under the id {key:#018x}, which is already held"
                );
            }
            Record::MadeChild(child) => {
                if let Some(entry) = shard.get_mut(&key) {
                    entry.made.insert(child);
                }
            }
            Record::Ended => {
                shard.remove(&key);
            }
        }
    }
}

/// A key and the record the view keeps under it.
type Keyed = (u64, Record);

/// The keys and records the view keeps of `effect`: one for the actor it makes or ends, and for a spawn a second,
/// by which the parent notes the child it made. None for an effect that does neither.
fn records(effect: &Effect) -> (Option<Keyed>, Option<Keyed>) {
    let by = effect.by();
    let seat = effect.seated().seat;
    let made = match effect.what() {
        What::Root { from } => Record::Made { above: None, segment: from.singleton(), seat, declared: from.declared() },
        What::RootInstanced { from, key } => {
            Record::Made { above: None, segment: from.instanced(key), seat, declared: from.declared() }
        }
        What::Failed(_) | What::Close => Record::Ended,
        _ => {
            let Some((child, segment)) = spawned(effect) else {
                return (None, None);
            };
            let Some(declared) = made_from(effect) else {
                return (None, None);
            };
            let record = Record::Made { above: Some(by), segment, seat: child.seat, declared };

            return (Some((child.id.bits(), record)), Some((by.bits(), Record::MadeChild(child.id.bits()))));
        }
    };

    (Some((by.bits(), made)), None)
}

/// What the type of the child `effect` stands up declares, or none for an effect that stands up no child.
const fn made_from(effect: &Effect) -> Option<&'static Declared> {
    match effect.what() {
        What::Spawn { from, .. }
        | What::SpawnInstanced { from, .. }
        | What::Embed { from, .. }
        | What::EmbedInstanced { from, .. } => Some(from.declared()),
        _ => None,
    }
}
