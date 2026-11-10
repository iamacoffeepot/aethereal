//! The engine's memory against its limits, kept as it folds and read between folds (ADR-0053).
//!
//! What a read answers is bounded by the parts, not the actors: the blob store keeps its own total, each part of the
//! [`Unsent`] view ranks its own actors as it folds, and each part's guests are measured as the part is put back
//! after it runs, so a read merges at most [`LARGEST`] of each from each part.

mod guests;
#[cfg(test)]
mod tests;

use crate::unsent::{self, Unsent};
use crate::{Blobs, Folded};
use aethereal_actor::{ActorName, Against, Backlog, Figures, GuestMemory, Instances, LARGEST, Process};
use aethereal_data::ActorId;
use guests::PartGuests;
use std::num::NonZeroUsize;

/// What the engine keeps for its figures: how its chassis reads the program's memory, each part's guests as last
/// measured, and the view of what each actor holds unsent, which the engine folds as it folds the others (ADR-0053).
///
/// A turn reads all of it through one reference, as the table it reads is built for each turn.
pub struct Stats {
    /// How the chassis reads the program's memory: [`Process::Unknown`] until it says.
    process: fn() -> Process,
    /// Each part's guests, as measured when the part was last put back.
    guests: Vec<PartGuests>,
    /// The bytes each actor that sends holds unsent, as it last said.
    unsent: Folded<Unsent>,
}

impl Stats {
    /// Nothing measured, in `parts` parts, the program's memory unknown.
    pub fn new(parts: NonZeroUsize) -> Self {
        Self {
            process: unknown,
            guests: (0..parts.get()).map(|_| PartGuests::default()).collect(),
            unsent: Folded::new(parts),
        }
    }

    /// The view of what each actor holds unsent.
    pub const fn unsent(&self) -> &Folded<Unsent> {
        &self.unsent
    }

    /// The view of what each actor holds unsent, to fold into.
    pub const fn unsent_mut(&mut self) -> &mut Folded<Unsent> {
        &mut self.unsent
    }

    /// Reads the program's memory with `read` from now on: the chassis's own way of asking.
    pub fn set_process(&mut self, read: fn() -> Process) {
        self.process = read;
    }

    /// Measures the guests of part `part` in `instances`, its instances as the part is put back.
    pub fn measure(&mut self, part: usize, instances: &Instances) {
        if let Some(guests) = self.guests.get_mut(part) {
            guests.measure(instances);
        }
    }

    /// The figures as they stand: the program's memory as its chassis reads it now, the blobs `blobs` holds, the
    /// backlogs the view ranks, and the guests last measured, each actor named by `name`.
    pub fn figures(&self, blobs: &Blobs, name: impl Fn(ActorId) -> ActorName) -> Figures {
        let (_, held) = blobs.held();
        let blobs = Against { bytes: wide(held), most: wide(blobs.budget()) };

        let mut guests: Vec<_> = self.guests.iter().flat_map(PartGuests::largest).collect();
        guests.sort_unstable_by(|a, b| b.bytes.cmp(&a.bytes).then(b.first.bits().cmp(&a.first.bits())));
        guests.truncate(LARGEST);
        let largest_guests = guests
            .into_iter()
            .map(|guest| GuestMemory { actor: name(guest.first), bytes: guest.bytes, actors: guest.actors })
            .collect();

        let mut backlogs: Vec<_> = self.unsent.shards().flat_map(unsent::Shard::longest).collect();
        backlogs.sort_unstable_by(|a, b| b.1.bytes.cmp(&a.1.bytes).then(b.0.bits().cmp(&a.0.bits())));
        backlogs.truncate(LARGEST);
        let longest_backlogs = backlogs.into_iter().map(|(id, unsent)| Backlog { actor: name(id), unsent }).collect();

        Figures {
            process: (self.process)(),
            blobs,
            guest_bytes: self.guests.iter().map(PartGuests::bytes).sum(),
            largest_guests,
            longest_backlogs,
        }
    }
}

/// The program's memory, as a chassis that has not said how to read it reads it.
const fn unknown() -> Process {
    Process::Unknown
}

/// `bytes` as a figure: the most a `u64` holds, on a host whose `usize` holds more.
fn wide(bytes: usize) -> u64 {
    u64::try_from(bytes).unwrap_or(u64::MAX)
}
