//! The buffers a turn's actor keeps, as its turn's pool, and a buffer the pool hands out, which goes back to it as it
//! drops unless it is put (ADR-0042).

#[cfg(feature = "std")]
use crate::Fills;
use crate::Spares;
use alloc::vec::Vec;
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

    /// A buffer with room for at least `room` bytes, to fill and put: the smallest the actor keeps that is big enough
    /// and at most twice `room`, emptied, else a new one. Put, its buffer comes back to the actor once the blob is
    /// freed; dropped unput, it comes back as it drops, so an actor allocates only when it keeps none big enough,
    /// whether it sends what it builds or only works in it (ADR-0042).
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
