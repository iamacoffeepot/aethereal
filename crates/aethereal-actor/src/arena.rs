//! Where the bytes of the mail a turn sends are written: an arena of the worker that ran it (ADR-0011).

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::mem;

/// How many bytes of mail one arena holds. A piece of mail that does not fit is held in bytes of its own.
pub const ARENA_BYTES: usize = 64 * 1024;

/// The bytes of the mail one writer sends in a step, written one after another into a slab of
/// [`ARENA_BYTES`] that never grows (ADR-0011).
///
/// A piece of mail is handled in the step after the one it is sent in, or
/// dropped, so its bytes are read in that next step and never after. The
/// engine keeps two arenas for each writer, one for each parity of the step,
/// and clears an arena when the writer's turn to write it comes round again:
/// nothing in it is freed, and no reader counts what it holds. A piece of mail
/// that does not fit in what is left of the slab is held in bytes of its own,
/// which go when the mail does.
#[derive(Debug, Default)]
pub struct Arena {
    /// Which writer's arena this is, as the engine numbers them.
    slot: usize,
    slab: Vec<u8>,
}

/// Where the bytes of one piece of mail are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Payload {
    /// In a writer's arena.
    InArena(Span),
    /// In bytes of its own, as it did not fit.
    Owned(Box<[u8]>),
}

/// A range of one writer's arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// The writer whose arena holds the bytes.
    pub slot: usize,
    /// Where they begin in it.
    pub start: usize,
    /// How many there are.
    pub len: usize,
}

impl Arena {
    /// The empty arena of the writer numbered `slot`. Its slab is allocated when it is first written to.
    #[must_use]
    pub const fn new(slot: usize) -> Self {
        Self { slot, slab: Vec::new() }
    }

    /// Which writer's arena this is.
    #[must_use]
    pub const fn slot(&self) -> usize {
        self.slot
    }

    /// Empties the arena, keeping its slab: for when its writer writes it again.
    pub fn clear(&mut self) {
        self.slab.clear();
    }

    /// The bytes at `span`, when it is a span of this arena; nothing otherwise.
    #[must_use]
    pub fn read(&self, span: Span) -> Option<&[u8]> {
        let is_own = span.slot == self.slot;
        let end = span.start.checked_add(span.len)?;

        is_own.then(|| self.slab.get(span.start..end)).flatten()
    }

    /// Writes one piece of mail with `write`, which appends its bytes to the vector it is given, and answers
    /// with where they are: in the slab when they fit, in bytes of their own otherwise. Nothing is written
    /// when `write` fails.
    pub(crate) fn write<E>(&mut self, write: impl FnOnce(Vec<u8>) -> (Vec<u8>, Result<(), E>)) -> Result<Payload, E> {
        let is_full = self.slab.len() >= ARENA_BYTES;
        if is_full {
            let (bytes, written) = write(Vec::new());
            written?;

            return Ok(Payload::Owned(bytes.into_boxed_slice()));
        }

        if self.slab.capacity() < ARENA_BYTES {
            self.slab.reserve_exact(ARENA_BYTES - self.slab.len());
        }
        let start = self.slab.len();
        let (slab, written) = write(mem::take(&mut self.slab));
        self.slab = slab;
        if let Err(error) = written {
            self.slab.truncate(start);
            return Err(error);
        }

        let fits = self.slab.len() <= ARENA_BYTES;
        if fits {
            return Ok(Payload::InArena(Span { slot: self.slot, start, len: self.slab.len() - start }));
        }
        // It ran past the slab: its bytes go to their own, and the slab returns to its size.
        let own = self.slab.split_off(start);
        self.slab.shrink_to(ARENA_BYTES);

        Ok(Payload::Owned(own.into_boxed_slice()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// Writes `bytes` into `arena` as one piece of mail.
    fn put(arena: &mut Arena, bytes: &[u8]) -> Payload {
        let Ok(payload) = arena.write::<()>(|mut out| {
            out.extend_from_slice(bytes);

            (out, Ok(()))
        }) else {
            panic!("an append does not fail");
        };

        payload
    }

    #[test]
    fn mail_past_the_slab_is_held_in_its_own_bytes_and_the_slab_stays_its_size() {
        // Catches mail that runs past the slab cut short or lost, a slab left grown by it, and mail after a full
        // slab written into it anyway.
        let mut arena = Arena::new(3);
        let first = put(&mut arena, &vec![1; ARENA_BYTES - 8]);
        let Payload::InArena(span) = first else {
            panic!("the first piece of mail fits");
        };
        assert_eq!(arena.read(span), Some(&vec![1; ARENA_BYTES - 8][..]));

        assert_eq!(put(&mut arena, &[2; 16]), Payload::Owned(vec![2; 16].into_boxed_slice()));
        assert_eq!(arena.slab.capacity(), ARENA_BYTES);
        assert_eq!(arena.read(span), Some(&vec![1; ARENA_BYTES - 8][..]));

        let Payload::InArena(last) = put(&mut arena, &[3; 8]) else {
            panic!("what is left of the slab holds a piece of mail that fits it");
        };
        assert_eq!(arena.read(last), Some(&[3; 8][..]));
        assert_eq!(put(&mut arena, &[4]), Payload::Owned(vec![4].into_boxed_slice()));
    }
}
