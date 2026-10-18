//! The children a step's turns stood up, seated where their seats lie when the part is folded (ADR-0011).

use super::made::Made;
use super::{Held, Seating, Sitting, part};
use crate::BySeat;
use aethereal_actor::{Above, Runs, Seated, Stood};
use aethereal_data::{ActorId, ActorPath};
use std::mem;

/// What one child a turn stood up asks of a part: to be seated in it, or to be noted in its parent's seat there.
pub(super) enum Birth {
    /// The child the actor `me` names, at `path`, under `above`, to seat.
    Child { me: Seated, path: ActorPath, above: Above, actor: Box<dyn Runs> },
    /// The child `child` the actor `parent` made, to note in the parent's seat (ADR-0008).
    Made { parent: Seated, child: ActorId },
}

/// One writer's births in a step, each in the bucket of the part it is for: the part of the child's seat, and of
/// its parent's.
#[derive(Default)]
pub(super) struct Births {
    /// A bucket for each part.
    buckets: Vec<Vec<Birth>>,
}

impl Births {
    /// An empty row of buckets, one for each of `parts` parts.
    pub(super) fn of_parts(parts: usize) -> Self {
        Self { buckets: (0..parts).map(|_| Vec::new()).collect() }
    }

    /// How many births this row holds for the part numbered `part`.
    pub(super) fn records(&self, part: usize) -> usize {
        self.buckets[part].len()
    }

    /// Puts the children one turn stood up, `stood`, in the buckets of their parts, each with the note of it for its
    /// parent's part.
    ///
    /// A child is stood up after the children its `begin` stood up, so they are taken from the back: a parent goes
    /// into its bucket before the note of any child of it, and is seated first.
    pub(super) fn stand(&mut self, stood: Vec<Stood>) {
        let parts = self.buckets.len();
        for child in stood.into_iter().rev() {
            let (me, path, above, actor) = child.into_parts();
            if let Above::Actor(parent, _) = above {
                self.buckets[part(parent.seat, parts)].push(Birth::Made { parent, child: me.id });
            }
            self.buckets[part(me.seat, parts)].push(Birth::Child { me, path, above, actor });
        }
    }

    /// Takes the bucket of the part numbered `part` out, leaving an empty one in its place, to fold.
    pub(super) fn take(&mut self, part: usize) -> Vec<Birth> {
        mem::take(&mut self.buckets[part])
    }

    /// Puts back the bucket of the part numbered `part`, emptied, for its room.
    pub(super) fn give_back(&mut self, part: usize, bucket: Vec<Birth>) {
        self.buckets[part] = bucket;
    }

    /// The bucket of the part numbered `part`, to fold where it lies.
    pub(super) fn bucket_mut(&mut self, part: usize) -> &mut Vec<Birth> {
        &mut self.buckets[part]
    }
}

/// Folds one writer's births for a part, in the order the writer put them, into its seats and actors, leaves the
/// bucket empty, with its room, and answers with how many children it seated.
pub(super) fn seat_all(seating: &mut Seating, actors: &mut BySeat<Held>, bucket: &mut Vec<Birth>) -> usize {
    let mut seated = 0;
    for birth in bucket.drain(..) {
        match birth {
            Birth::Child { me, path, above, actor } => {
                sit(seating, actors, me, path, above, actor);
                seated += 1;
            }
            Birth::Made { parent, child } => note(seating, parent, child),
        }
    }

    seated
}

/// Seats the actor `me` names, at `path`, under `above`, in its part's `seating` and `actors`.
pub(super) fn sit(
    seating: &mut Seating,
    actors: &mut BySeat<Held>,
    me: Seated,
    path: ActorPath,
    above: Above,
    actor: Box<dyn Runs>,
) {
    let kind = actor.declared().id();
    let prepares = actor.prepares();
    seating.seated.insert(me, Sitting { path, above, kind, made: Made::none(), prepares });
    actors.insert(me, Held::Here(actor));
}

/// Notes in the seat of `parent`, in its part's `seating`, that it made the child `child`. A parent that has left
/// its seat has no keys to keep.
pub(super) fn note(seating: &mut Seating, parent: Seated, child: ActorId) {
    if let Some(sitting) = seating.seated.get_mut(parent) {
        sitting.made.insert(child);
    }
}
