//! The buffers an actor keeps to put blobs in again: each the buffer of a blob it put, handed back once the blob was
//! freed (ADR-0042).

use alloc::vec::Vec;

/// How many times the room asked a kept buffer may hold, and still be handed out: a `Vec` grows by doubling, so one
/// grown to hold some bytes holds less than twice them.
const FITS: usize = 2;

/// The buffers an actor keeps, emptied, to hand out again for its next puts, and the bytes they hold between them
/// (ADR-0042).
///
/// It lives beside the actor in what the engine keeps for it, so only the
/// thread that holds the actor's part reaches it.
#[derive(Default)]
pub struct Spares {
    /// Each buffer kept, emptied, smallest capacity first.
    buffers: Vec<Vec<u8>>,
    /// The capacity of every buffer kept, in bytes.
    bytes: usize,
}

impl Spares {
    /// A buffer with room for at least `room` bytes: the smallest kept that is big enough and at most twice `room`,
    /// else a new one. A buffer is charged to the budget by its capacity, so a small put is never handed a large one.
    pub fn take(&mut self, room: usize) -> Vec<u8> {
        let first = self.buffers.partition_point(|buffer| buffer.capacity() < room);
        let most = room.saturating_mul(FITS);
        let fits = self.buffers.get(first).is_some_and(|buffer| buffer.capacity() <= most);
        if !fits {
            return Vec::with_capacity(room);
        }
        let buffer = self.buffers.remove(first);
        self.bytes -= buffer.capacity();

        buffer
    }

    /// Keeps `buffer`, emptied, to hand out again: dropped instead when it holds nothing, or keeping it would take the
    /// buffers kept past `most_bytes`.
    pub fn keep(&mut self, mut buffer: Vec<u8>, most_bytes: usize) {
        let capacity = buffer.capacity();
        let holds_nothing = capacity == 0;
        let overflows = self.bytes.saturating_add(capacity) > most_bytes;
        if holds_nothing || overflows {
            return;
        }
        buffer.clear();
        let at = self.buffers.partition_point(|kept| kept.capacity() < capacity);
        self.buffers.insert(at, buffer);
        self.bytes += capacity;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_smallest_buffer_big_enough_is_handed_out() {
        // Catches a buffer handed out that is too small for the room asked, and a larger one handed out where a
        // smaller one kept would do, which leaves the larger one for no later put that needs it.
        let mut spares = Spares::default();
        spares.keep(Vec::with_capacity(8192), usize::MAX);
        spares.keep(Vec::with_capacity(4096), usize::MAX);

        let taken = [spares.take(3000), spares.take(5000)].map(|buffer| buffer.capacity());

        assert_eq!(taken, [4096, 8192]);
    }

    #[test]
    fn a_buffer_more_than_twice_the_room_is_not_handed_out() {
        // Catches a small put handed a large kept buffer, which the budget would charge in full.
        let mut spares = Spares::default();
        spares.keep(Vec::with_capacity(4096), usize::MAX);

        let small = spares.take(100).capacity();
        let kept = spares.take(2048).capacity();

        assert_eq!([small, kept], [100, 4096]);
    }
}
