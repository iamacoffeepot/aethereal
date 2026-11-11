//! The view of which actors there are (ADR-0008, ADR-0011).

use crate::id::spawned;
use crate::log::{MOST_RECENT, share};
use crate::{ByActor, Context, Part, View};
use aethereal_actor::{Arena, Declared, Effect, SeatNumber, What};
use aethereal_data::{ActorId, PathSegment};
use std::collections::VecDeque;
use std::num::NonZeroUsize;

/// The view of which actors are live, keyed by an actor's own id (ADR-0011).
///
/// A key has one source for each of its two changes. An actor's entry is made
/// by the spawn its parent asked for, or for a root by the chassis. It is
/// taken out by the actor's own ending and by nothing else: no ending of
/// another actor, its parent's included, reaches it (ADR-0009).
pub struct Actors;

/// One part of the table: the live actors whose ids fall in it, and the entries of those that ended most recently.
pub struct Standing {
    /// The live actors.
    live: ByActor<Entry>,
    /// The entries of the actors that ended most recently, kept to name them (ADR-0053).
    ended: EndedNames,
}

impl Part for Standing {
    fn empty(parts: NonZeroUsize) -> Self {
        let ended = EndedNames { entries: ByActor::default(), order: VecDeque::new(), most: share(MOST_RECENT, parts) };

        Self { live: ByActor::default(), ended }
    }
}

impl Standing {
    /// The entry of the live actor `key` is the id of.
    pub(super) fn live(&self, key: u64) -> Option<&Entry> {
        self.live.get(&key)
    }

    /// The entry of the actor `key` is the id of, live or among those that ended most recently.
    pub(super) fn named(&self, key: u64) -> Option<&Entry> {
        self.live.get(&key).or_else(|| self.ended.entries.get(&key))
    }

    /// Notes that the live actor `key` is the id of runs code whose type `declared` declares from now on (ADR-0054).
    pub(super) fn moved(&mut self, key: u64, declared: &'static Declared) {
        if let Some(entry) = self.live.get_mut(&key) {
            entry.declared = declared;
        }
    }

    /// Keeps the entries of at most `most` actors that ended, the most recent: for the engine's setup, before any fold
    /// (ADR-0053).
    pub(super) const fn keep_ended_at_most(&mut self, most: usize) {
        self.ended.most = most;
    }
}

/// The entries of the actors of a part that ended most recently, as many as the log's ring keeps entries, the oldest
/// forgotten as a newer one ends: so the entries a log keeps across actors are named by path once their actor has
/// ended, as the one that says why it ended is (ADR-0053).
struct EndedNames {
    entries: ByActor<Entry>,
    /// The ids kept, in the order their actors ended, oldest first.
    order: VecDeque<u64>,
    /// The most entries kept: fixed as the engine is set up, before any fold.
    most: usize,
}

impl EndedNames {
    /// Keeps `entry`, of the actor `key` is the id of, which has ended, forgetting the oldest past the most kept. An
    /// id names one path, so an actor that ends again under an id kept keeps its place.
    fn keep(&mut self, key: u64, entry: Entry) {
        if self.most == 0 {
            return;
        }
        let was_kept = self.entries.insert(key, entry).is_some();
        if was_kept {
            return;
        }
        let is_full = self.order.len() >= self.most;
        if is_full && let Some(oldest) = self.order.pop_front() {
            self.entries.remove(&oldest);
        }
        self.order.push_back(key);
    }
}

/// One live actor's entry: where it stands.
pub struct Entry {
    /// The actor directly above. None for a root, which stands beneath nothing: fixed when the entry is made.
    above: Option<ActorId>,
    /// The actor's own segment of its path.
    segment: PathSegment,
    /// The seat the actor sits in, or sat in once it has gone.
    seat: SeatNumber,
    /// What the actor's type declares (ADR-0013).
    declared: &'static Declared,
}

/// What the view keeps of an effect.
pub enum Record {
    /// An actor of the type `declared` describes was made at `segment`, beneath `above` where it is not a root, in
    /// `seat`.
    Made { above: Option<ActorId>, segment: PathSegment, seat: SeatNumber, declared: &'static Declared },
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
}

impl View for Actors {
    type Shard = Standing;
    type Record = Record;

    fn takes<'a>(effect: &'a Effect, _: &'a Context) -> impl Iterator<Item = (u64, Record)> + 'a {
        record(effect).into_iter()
    }

    /// # Panics
    ///
    /// Panics when an actor is made under an id the table already holds (ADR-0008). A parent refuses a key it has
    /// used before it asks, so that is two paths with one id or a fault in the engine, and neither has a way on.
    fn fold(shard: &mut Standing, key: u64, record: Record, _: &[Arena]) {
        match record {
            Record::Made { above, segment, seat, declared } => {
                let held = shard.live.insert(key, Entry { above, segment, seat, declared });
                assert!(
                    held.is_none(),
                    "actor table: an actor was made under the id {key:#018x}, which is already held"
                );
            }
            Record::Ended => {
                if let Some(entry) = shard.live.remove(&key) {
                    shard.ended.keep(key, entry);
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
        What::Root { from } => {
            (by, Record::Made { above: None, segment: from.singleton(), seat, declared: from.declared() })
        }
        What::RootInstanced { from, key } => {
            (by, Record::Made { above: None, segment: from.instanced(key), seat, declared: from.declared() })
        }
        What::Failed(_) | What::Close => (by, Record::Ended),
        _ => {
            let (child, segment) = spawned(effect)?;
            let declared = made_from(effect)?;

            (child.id, Record::Made { above: Some(by), segment, seat: child.seat, declared })
        }
    };

    Some((id.bits(), record))
}

/// What the type of the child `effect` stands up declares, or none for an effect that stands up no child.
const fn made_from(effect: &Effect) -> Option<&'static Declared> {
    match effect.what() {
        What::Spawn { from, .. } | What::SpawnInstanced { from, .. } => Some(from.declared()),
        _ => None,
    }
}
