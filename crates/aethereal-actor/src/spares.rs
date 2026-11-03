//! The buffers an actor keeps to put blobs in again: each the buffer of a blob it put, handed back once the blob was
//! freed, and the buffer a job it runs to fill one is lent (ADR-0042).

use alloc::vec;
use alloc::vec::Vec;

/// How many times the room asked a kept buffer may hold, and still be handed out: a `Vec` grows by doubling, so one
/// grown to hold some bytes holds less than twice them.
const FITS: usize = 2;

/// The buffers an actor keeps, emptied, to hand out again for its next puts, and the bytes they hold between them
/// (ADR-0042). A job that fills buffers holds the one it was lent the same way, and hands it out by the same rule.
///
/// It lives beside the actor in what the engine keeps for it, so only the
/// thread that holds the actor's part reaches it, or with the job it was lent
/// to, which only the thread running the job reaches.
#[derive(Debug, Default)]
pub struct Spares {
    /// Each buffer kept, emptied, smallest capacity first.
    buffers: Vec<Vec<u8>>,
    /// The capacity of every buffer kept, in bytes.
    bytes: usize,
}

impl Spares {
    /// None kept.
    #[must_use]
    pub const fn new() -> Self {
        Self { buffers: Vec::new(), bytes: 0 }
    }

    /// Whether none is kept.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buffers.is_empty()
    }

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

    /// Lends out the largest buffer kept, to a job that fills buffers, as what that job holds: none when none is kept.
    /// The largest fits the most of what the job may fill, and the job takes it by the rule [`Self::take`] keeps.
    #[must_use]
    pub fn lend(&mut self) -> Self {
        let Some(largest) = self.buffers.pop() else {
            return Self::new();
        };
        let bytes = largest.capacity();
        self.bytes -= bytes;

        Self { buffers: vec![largest], bytes }
    }

    /// Keeps each of `spares`, as [`Self::keep`] keeps one: the buffers a job was lent and did not take, back with
    /// the actor that ran it.
    pub fn keep_all(&mut self, spares: Self, most_bytes: usize) {
        for buffer in spares.buffers {
            self.keep(buffer, most_bytes);
        }
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

    #[test]
    fn the_largest_buffer_is_lent_and_no_longer_counted_as_kept() {
        // Catches a job lent a smaller buffer than the actor keeps, which a large read could not fill, and the lent
        // buffer's bytes still counted against what the actor keeps, which would drop the next buffer handed back.
        let mut spares = Spares::default();
        spares.keep(Vec::with_capacity(8192), usize::MAX);
        spares.keep(Vec::with_capacity(4096), usize::MAX);

        let mut lent = spares.lend();
        spares.keep(Vec::with_capacity(4096), 8192);

        let taken = [lent.take(5000), spares.take(3000), spares.take(3000)].map(|buffer| buffer.capacity());
        assert_eq!(taken, [8192, 4096, 4096]);
    }
}
