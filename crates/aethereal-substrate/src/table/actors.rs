//! The view of which actors there are (ADR-0008, ADR-0011).

use crate::id::child_id;
use crate::{Effect, View, What};
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

    fn takes(effect: &Effect) -> Option<(u64, Record)> {
        let by = effect.by();
        let (id, record) = match effect.what() {
            What::Root { from } => (by, Record::Made { above: None, segment: from.singleton() }),
            What::RootInstanced { from, key } => (by, Record::Made { above: None, segment: from.instanced(key) }),
            What::Spawn { from } | What::Embed { from } => beneath(by, from.singleton()),
            What::SpawnInstanced { from, key } | What::EmbedInstanced { from, key } => beneath(by, from.instanced(key)),
            What::Failed | What::Close => (by, Record::Ended),
        };

        Some((id.bits(), record))
    }

    /// An id already in the table keeps the entry it has: a path names one actor for the engine's whole life
    /// (ADR-0008), so a spawn at a used key is refused where it is asked for and never reaches here as a change.
    fn fold(shard: &mut BTreeMap<u64, Entry>, key: u64, record: Record) {
        match record {
            Record::Made { above, segment } => {
                shard.entry(key).or_insert(Entry { above, segment, life: Life::Live });
            }
            Record::Ended => {
                if let Some(entry) = shard.get_mut(&key) {
                    entry.life = Life::Ended;
                }
            }
        }
    }
}

/// The id and record of the child `segment` names beneath the actor `parent` stands for.
fn beneath(parent: ActorId, segment: PathSegment) -> (ActorId, Record) {
    (child_id(parent, &segment), Record::Made { above: Some(parent), segment })
}
