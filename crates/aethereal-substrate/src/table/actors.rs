//! The view of which actors there are (ADR-0008, ADR-0011).

use crate::View;
use crate::id::spawned;
use aethereal_actor::{Effect, What};
use aethereal_data::{ActorId, PathSegment};
use std::collections::BTreeMap;

/// The view of which actors there are, keyed by an actor's own id (ADR-0011).
///
/// A key has one source for each of its two changes. An actor's entry is made
/// by the spawn its parent asked for, or for a root by the chassis. It is
/// marked ended by the actor's own ending and by nothing else: no ending of
/// another actor, its parent's included, reaches it (ADR-0009).
pub(super) struct Actors;

/// One actor's entry: where it stands, and whether it has ended.
pub(super) struct Entry {
    /// The actor directly above. None for a root, which stands beneath nothing: fixed when the entry is made.
    above: Option<ActorId>,
    /// The actor's own segment of its path.
    segment: PathSegment,
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
pub(super) enum Record {
    /// An actor was made at `segment`, beneath `above` where it is not a root.
    Made { above: Option<ActorId>, segment: PathSegment },
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

    /// Whether the actor has not ended.
    pub(super) fn is_live(&self) -> bool {
        self.life == Life::Live
    }
}

impl View for Actors {
    type Shard = BTreeMap<u64, Entry>;
    type Record = Record;
    type Reads<'a> = ();

    fn takes(effect: &Effect, (): ()) -> impl Iterator<Item = (u64, Record)> + '_ {
        record(effect).into_iter()
    }

    /// # Panics
    ///
    /// Panics when an actor is made under an id the table already holds (ADR-0008). A parent refuses a key it has
    /// used before it asks, so that is two paths with one id or a fault in the engine, and neither has a way on.
    fn fold(shard: &mut BTreeMap<u64, Entry>, key: u64, record: Record) {
        match record {
            Record::Made { above, segment } => {
                let held = shard.insert(key, Entry { above, segment, life: Life::Live });
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
    let (id, record) = match effect.what() {
        What::Root { from } => (by, Record::Made { above: None, segment: from.singleton() }),
        What::RootInstanced { from, key } => (by, Record::Made { above: None, segment: from.instanced(key) }),
        What::Failed | What::Close => (by, Record::Ended),
        _ => {
            let (child, segment) = spawned(effect)?;

            (child, Record::Made { above: Some(by), segment })
        }
    };

    Some((id.bits(), record))
}
