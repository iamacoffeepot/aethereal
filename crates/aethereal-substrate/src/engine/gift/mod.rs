//! The resources a step's turns gave, carried to the parts of the actors their mail goes to, and handed to them when
//! the part is folded (ADR-0010).

use super::{Held, part};
use crate::BySeat;
use aethereal_actor::{Kept, MailId, Seated};
use std::mem;

/// One resource on its way to the actor `to`, given by the actor `by` with the mail `mail`.
pub(super) struct Arrival {
    pub(super) to: Seated,
    pub(super) by: Seated,
    pub(super) mail: MailId,
    pub(super) kept: Kept,
}

/// One writer's resources on their way in a step, each in the bucket of the part its receiver sits in.
#[derive(Default)]
pub(super) struct Gifts {
    /// A bucket for each part.
    buckets: Vec<Vec<Arrival>>,
}

impl Gifts {
    /// An empty row of buckets, one for each of `parts` parts.
    pub(super) fn of_parts(parts: usize) -> Self {
        Self { buckets: (0..parts).map(|_| Vec::new()).collect() }
    }

    /// How many resources this row carries to the part numbered `part`.
    pub(super) fn records(&self, part: usize) -> usize {
        self.buckets[part].len()
    }

    /// Puts `arrival` in the bucket of its receiver's part.
    pub(super) fn put(&mut self, arrival: Arrival) {
        let parts = self.buckets.len();
        self.buckets[part(arrival.to.seat, parts)].push(arrival);
    }

    /// Takes the bucket of the part numbered `part` out, leaving an empty one in its place, to fold.
    pub(super) fn take(&mut self, part: usize) -> Vec<Arrival> {
        mem::take(&mut self.buckets[part])
    }

    /// Puts back the bucket of the part numbered `part`, emptied, for its room.
    pub(super) fn give_back(&mut self, part: usize, bucket: Vec<Arrival>) {
        self.buckets[part] = bucket;
    }

    /// The bucket of the part numbered `part`, to fold where it lies.
    pub(super) fn bucket_mut(&mut self, part: usize) -> &mut Vec<Arrival> {
        &mut self.buckets[part]
    }
}

/// Hands each resource in `bucket` to the actor its mail goes to, among `actors`, in the order the writer put them,
/// and leaves the bucket empty, with its room. One whose receiver no longer sits there is dropped, and its giver and
/// name noted in `lost`, for the giver's log.
pub(super) fn arrive_all(actors: &mut BySeat<Held>, bucket: &mut Vec<Arrival>, lost: &mut Vec<(Seated, &'static str)>) {
    for Arrival { to, by, mail, kept } in bucket.drain(..) {
        match actors.get_mut(to) {
            Some(held) => held.keeping.arrive(by, mail, kept),
            None => lost.push((by, kept.name())),
        }
    }
}

#[cfg(test)]
mod tests;
