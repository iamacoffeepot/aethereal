//! The buffers a turn's actor keeps, as its turn's pool, and a buffer the pool hands out, which goes back to it as it
//! drops unless it is put, or several handed out together, which go back together (ADR-0042).

#[cfg(feature = "std")]
use crate::Fills;
use crate::Spares;
use alloc::vec::Vec;
use core::array;
use core::fmt;
use core::mem;
use core::ops::{Deref, DerefMut};

/// The buffers a turn's actor keeps, borrowed apart from the rest of the turn with
/// [`Turn::split`](crate::Turn::split): it hands out a [`Buffer`] to fill and put, which borrows the pool and not the
/// turn (ADR-0042).
pub struct Pool<'a> {
    back: Back<'a>,
}

/// Where the buffers a pool hands out come from and go back to.
enum Back<'a> {
    /// The buffers the engine keeps for the actor, which hold at most `most_bytes` between them.
    To { spares: &'a mut Spares, most_bytes: usize },
    /// None: a guest's code keeps none of the engine's buffers, so each is made new and freed as it drops.
    Nowhere,
}

impl<'a> Pool<'a> {
    /// The pool of `spares`, the buffers the engine keeps for an actor, which keeps at most `most_bytes` of them.
    pub(crate) const fn kept(spares: &'a mut Spares, most_bytes: usize) -> Self {
        Self { back: Back::To { spares, most_bytes } }
    }

    /// The pool of a guest's code, which keeps none of the engine's buffers.
    pub(crate) const fn unkept() -> Self {
        Self { back: Back::Nowhere }
    }

    /// The same pool, lent for a shorter while: what it hands out goes back to where this one's does.
    pub(crate) fn reborrow(&mut self) -> Pool<'_> {
        let back = match &mut self.back {
            Back::To { spares, most_bytes } => Back::To { spares, most_bytes: *most_bytes },
            Back::Nowhere => Back::Nowhere,
        };

        Pool { back }
    }

    /// A buffer with room for at least `room` bytes, to fill and put: the smallest the actor keeps that is big enough
    /// and at most twice `room`, emptied, else a new one with room for `room`, as these are bytes the caller will
    /// write. Put, its buffer comes back to the actor once the blob is freed; dropped unput, it comes back as it drops,
    /// so an actor allocates only when it keeps none big enough, whether it sends what it builds or only works in it
    /// (ADR-0042).
    #[must_use]
    pub fn buffer(&mut self, room: usize) -> Buffer<'_> {
        match &mut self.back {
            Back::To { spares, most_bytes } => {
                let bytes = spares.take(room);
                let back = Back::To { spares, most_bytes: *most_bytes };

                Buffer { bytes, back }
            }
            Back::Nowhere => Buffer { bytes: Vec::with_capacity(room), back: Back::Nowhere },
        }
    }

    /// `N` buffers held together, for work that reads one while it writes another: each the smallest the actor keeps
    /// that is big enough for `room` and at most twice it, emptied, else an empty one that allocates only as it is
    /// written, as these are bytes the work might write, so work that writes none allocates nothing. Each goes back to
    /// the actor as the guard drops, as a dropped [`Buffer`] does; one taken out of it to be put comes back once its
    /// blob is freed (ADR-0042).
    #[must_use]
    pub fn buffers<const N: usize>(&mut self, room: usize) -> BufferGroup<'_, N> {
        match &mut self.back {
            Back::To { spares, most_bytes } => {
                let bytes = array::from_fn(|_| spares.kept(room).unwrap_or_default());
                let back = Back::To { spares, most_bytes: *most_bytes };

                BufferGroup { bytes, back }
            }
            Back::Nowhere => BufferGroup { bytes: array::from_fn(|_| Vec::new()), back: Back::Nowhere },
        }
    }

    /// A buffer as [`Self::buffer`] hands out, bound to the buffers the pool borrows rather than to the pool itself:
    /// for a job, whose pool is made for each borrow of its buffers.
    #[cfg(feature = "std")]
    pub(crate) fn buffer_kept(self, room: usize) -> Buffer<'a> {
        match self.back {
            Back::To { spares, most_bytes } => {
                Buffer { bytes: spares.take(room), back: Back::To { spares, most_bytes } }
            }
            Back::Nowhere => Buffer { bytes: Vec::with_capacity(room), back: Back::Nowhere },
        }
    }

    /// What a job that `fills` is lent of the actor's buffers: its largest, for a job that fills buffers, and none
    /// otherwise, or when it keeps none (ADR-0042).
    #[cfg(feature = "std")]
    pub(crate) fn lend(&mut self, fills: Fills) -> Spares {
        match (fills, &mut self.back) {
            (Fills::Buffers, Back::To { spares, .. }) => spares.lend(),
            (Fills::Buffers | Fills::Nothing, _) => Spares::new(),
        }
    }
}

impl fmt::Debug for Pool<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pool").finish_non_exhaustive()
    }
}

/// A buffer a turn's [`Pool`] handed out, to fill and put: it reads and is written as the `Vec<u8>` it holds. Put, its
/// bytes become a blob, whose buffer comes back to the actor once the blob is freed; dropped unput, it goes back to
/// the actor at once, emptied, by the rule and under the budget a freed blob's buffer is kept by. So every buffer an
/// actor takes is the engine's to reuse, whether it is sent or only worked in (ADR-0042).
///
/// It borrows the pool, so it never outlives the turn and is never kept in the actor's state; a buffer that is to be
/// kept there is taken out of it with `Vec::from`, and is then the actor's own.
pub struct Buffer<'p> {
    bytes: Vec<u8>,
    back: Back<'p>,
}

impl Deref for Buffer<'_> {
    type Target = Vec<u8>;

    fn deref(&self) -> &Vec<u8> {
        &self.bytes
    }
}

impl DerefMut for Buffer<'_> {
    fn deref_mut(&mut self) -> &mut Vec<u8> {
        &mut self.bytes
    }
}

impl From<Buffer<'_>> for Vec<u8> {
    /// The bytes of `buffer`, taken out of it to be put or kept as the actor's own: the buffer then holds nothing, and
    /// gives nothing back as it drops.
    fn from(mut buffer: Buffer<'_>) -> Self {
        mem::take(&mut buffer.bytes)
    }
}

impl Drop for Buffer<'_> {
    /// Gives the buffer back to the actor's, emptied: dropped instead when it holds nothing, as one put or taken out
    /// does, or when keeping it would take them past what the actor may keep.
    fn drop(&mut self) {
        if let Back::To { spares, most_bytes } = &mut self.back {
            spares.keep(mem::take(&mut self.bytes), *most_bytes);
        }
    }
}

impl fmt::Debug for Buffer<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Buffer").field("len", &self.bytes.len()).field("capacity", &self.bytes.capacity()).finish()
    }
}

/// `N` buffers a turn's [`Pool`] handed out together, as one guard: it reads and is written as the `[Vec<u8>; N]` it
/// holds, so work can read one while it writes another. As it drops, each goes back to the actor, emptied, as a
/// dropped [`Buffer`] does; one taken out of it, with `mem::take`, to be put or kept, gives nothing back, and is then
/// the actor's own: its buffer comes back only once it is put (ADR-0042).
pub struct BufferGroup<'p, const N: usize> {
    bytes: [Vec<u8>; N],
    back: Back<'p>,
}

impl<const N: usize> Deref for BufferGroup<'_, N> {
    type Target = [Vec<u8>; N];

    fn deref(&self) -> &[Vec<u8>; N] {
        &self.bytes
    }
}

impl<const N: usize> DerefMut for BufferGroup<'_, N> {
    fn deref_mut(&mut self) -> &mut [Vec<u8>; N] {
        &mut self.bytes
    }
}

impl<const N: usize> Drop for BufferGroup<'_, N> {
    /// Gives each buffer back to the actor's, emptied, by the rule [`Buffer`] gives its one back by.
    fn drop(&mut self) {
        if let Back::To { spares, most_bytes } = &mut self.back {
            for bytes in &mut self.bytes {
                spares.keep(mem::take(bytes), *most_bytes);
            }
        }
    }
}

impl<const N: usize> fmt::Debug for BufferGroup<'_, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let capacities = self.bytes.each_ref().map(Vec::capacity);

        f.debug_struct("BufferGroup").field("capacities", &capacities).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_buffer_handed_out_together_goes_back_together() {
        // Catches a buffer of the guard not given back as it drops, which the next turn would allocate again, and two
        // slots handed the same kept buffer, or one kept buffer handed out twice.
        let mut spares = Spares::default();
        spares.keep(Vec::with_capacity(4096), usize::MAX);
        spares.keep(Vec::with_capacity(8192), usize::MAX);
        let mut pool = Pool::kept(&mut spares, usize::MAX);

        let first = pool.buffers::<2>(4096).each_ref().map(Vec::capacity);
        let again = pool.buffers::<2>(4096).each_ref().map(Vec::capacity);

        assert_eq!([first, again], [[4096, 8192], [4096, 8192]]);
    }

    #[test]
    fn a_buffer_the_pool_does_not_keep_is_handed_out_empty() {
        // Catches a slot the pool keeps no buffer for allocated up front, which a stack whose layers never write would
        // pay on every part.
        let mut spares = Spares::default();
        spares.keep(Vec::with_capacity(4096), usize::MAX);
        let mut pool = Pool::kept(&mut spares, usize::MAX);

        let capacities = pool.buffers::<2>(3000).each_ref().map(Vec::capacity);

        assert_eq!(capacities, [4096, 0]);
    }
}
