//! Each part's guests, measured as the part is put back after it runs, so a read names the largest without walking
//! the instances (ADR-0015, ADR-0053).

use aethereal_actor::{Instances, LARGEST, Memory};
use aethereal_data::ActorId;

/// One part's guests: the bytes of all their memory, and the largest of them, the most first.
#[derive(Default)]
pub struct PartGuests {
    bytes: u64,
    /// At most [`LARGEST`]: kept from one measure to the next for its room.
    largest: Vec<Measured>,
}

/// One guest as it was measured: the bytes of its memory, how many actors run in it, and the first of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measured {
    pub bytes: u64,
    pub actors: u32,
    pub first: ActorId,
}

impl PartGuests {
    /// Measures the guests among `instances` again, forgetting what was measured before. An instance that is no
    /// guest's, as every instance of an engine with no guests is, counts nothing.
    pub fn measure(&mut self, instances: &Instances) {
        self.bytes = 0;
        self.largest.clear();
        instances.measure(|memory| {
            let Memory::Guest { bytes, actors, first } = memory else {
                return;
            };
            self.bytes = self.bytes.saturating_add(bytes);
            let at = self.largest.partition_point(|kept| kept.bytes >= bytes);
            if at < LARGEST {
                self.largest.truncate(LARGEST - 1);
                self.largest.insert(at, Measured { bytes, actors, first: first.id });
            }
        });
    }

    /// The bytes of all the part's guests' memory.
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }

    /// The part's largest guests, the most first.
    pub fn largest(&self) -> impl Iterator<Item = Measured> + '_ {
        self.largest.iter().copied()
    }
}
