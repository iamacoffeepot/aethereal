//! Where the bytes of the mail a turn sends are written: an arena of the worker that ran it (ADR-0011).

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::mem;

/// The fewest bytes of mail one arena's slab holds: what it holds until the engine gives it its share of the host's
/// quota. A piece of mail that does not fit is spilled into bytes of its own.
pub const ARENA_BYTES: usize = 64 * 1024;

/// The bytes of the mail one writer sends in a step, written one after another into a slab that does not grow while
/// it is written (ADR-0011).
///
/// A piece of mail is handled in the step after the one it is sent in, or
/// dropped, so its bytes are read in that next step and never after. The
/// engine keeps two arenas for each writer, one for each parity of the step,
/// and clears an arena when the writer's turn to write it comes round again:
/// nothing in it is freed, and no reader counts what it holds. A piece of mail
/// that does not fit in what is left of the slab is spilled into bytes of its
/// own, which the arena keeps and frees when it is cleared, so where a piece of
/// mail is never owns anything and is dropped without a look.
#[derive(Debug)]
pub struct Arena {
    /// Which writer's arena this is, as the engine numbers them.
    slot: u32,
    slab: Vec<u8>,
    /// The mail that did not fit in the slab, each piece in bytes of its own, in the order written.
    spilled: Vec<Box<[u8]>>,
    /// How many bytes the slab holds: the writer's share of the host's quota for mail, at least [`ARENA_BYTES`] and
    /// at most [`SLAB_MOST`].
    bytes: usize,
}

/// The most bytes one arena's slab holds, so that where a piece of mail lies in it is a 32-bit number.
const SLAB_MOST: usize = u32::MAX as usize;

/// Where the bytes of one piece of mail are, in the arena of the writer that wrote them: sixteen bytes, carried
/// with every piece of mail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Payload {
    /// In the writer's slab.
    InArena(Span),
    /// Spilled, as it did not fit: the writer's `index`th spilled piece.
    Spilled { slot: u32, index: u32 },
}

/// A range of one writer's arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// The writer whose arena holds the bytes.
    pub slot: u32,
    /// Where they begin in it.
    pub start: u32,
    /// How many there are.
    pub len: u32,
}

impl Default for Arena {
    /// The empty arena of the first writer, its slab the fewest bytes.
    fn default() -> Self {
        Self::new(0)
    }
}

impl Arena {
    /// The empty arena of the writer numbered `slot`. Its slab is allocated when it is first written to.
    ///
    /// # Panics
    ///
    /// Panics for a writer numbered past `u32::MAX`, which no engine has.
    #[must_use]
    pub fn new(slot: usize) -> Self {
        let Ok(slot) = u32::try_from(slot) else {
            panic!("no engine has more than u32::MAX writers");
        };

        Self { slot, slab: Vec::new(), spilled: Vec::new(), bytes: ARENA_BYTES }
    }

    /// Which writer's arena this is.
    #[must_use]
    pub const fn slot(&self) -> usize {
        self.slot as usize
    }

    /// Empties the arena, keeping its slab and freeing what was spilled, for when its writer writes it again, and
    /// gives its slab `bytes`, at least [`ARENA_BYTES`] and at most [`SLAB_MOST`]: the writer's share of the host's
    /// quota for mail.
    pub fn clear(&mut self, bytes: usize) {
        self.slab.clear();
        self.spilled.clear();
        self.bytes = bytes.clamp(ARENA_BYTES, SLAB_MOST);
    }

    /// The bytes `payload` says where to find, when they are in this arena; nothing otherwise.
    #[must_use]
    pub fn read(&self, payload: Payload) -> Option<&[u8]> {
        let is_own = payload.slot() == self.slot();
        let bytes = match payload {
            Payload::InArena(Span { start, len, .. }) => {
                let start = start as usize;

                self.slab.get(start..start.checked_add(len as usize)?)
            }
            Payload::Spilled { index, .. } => self.spilled.get(index as usize).map(|bytes| &bytes[..]),
        };

        is_own.then_some(bytes).flatten()
    }

    /// Keeps `bytes`, which did not fit in the slab, as a spilled piece of mail.
    ///
    /// # Panics
    ///
    /// Panics past `u32::MAX` spilled pieces in one arena, each of which holds its own allocation.
    fn spill(&mut self, bytes: Vec<u8>) -> Payload {
        let Ok(index) = u32::try_from(self.spilled.len()) else {
            panic!("no arena spills more than u32::MAX pieces of mail");
        };
        self.spilled.push(bytes.into_boxed_slice());

        Payload::Spilled { slot: self.slot, index }
    }

    /// Writes one piece of mail with `write`, which appends its bytes to the vector it is given, and answers
    /// with where they are: in the slab when they fit, spilled otherwise. Nothing is written
    /// when `write` fails.
    pub(crate) fn write<E>(&mut self, write: impl FnOnce(Vec<u8>) -> (Vec<u8>, Result<(), E>)) -> Result<Payload, E> {
        let is_full = self.slab.len() >= self.bytes;
        if is_full {
            let (bytes, written) = write(Vec::new());
            written?;

            return Ok(self.spill(bytes));
        }

        if self.slab.capacity() < self.bytes {
            self.slab.reserve_exact(self.bytes - self.slab.len());
        }
        let start = self.slab.len();
        let (slab, written) = write(mem::take(&mut self.slab));
        self.slab = slab;
        if let Err(error) = written {
            self.slab.truncate(start);
            return Err(error);
        }

        // The slab holds at most `SLAB_MOST` bytes, so mail that fits it always has a span.
        let fits = self.slab.len() <= self.bytes;
        if fits && let Some(span) = Span::between(self.slot, start, self.slab.len()) {
            return Ok(Payload::InArena(span));
        }
        // It ran past the slab: its bytes are spilled, and the slab returns to its size.
        let own = self.slab.split_off(start);
        self.slab.shrink_to(self.bytes);

        Ok(self.spill(own))
    }
}

impl Span {
    /// The bytes from `start` to `end` of the arena of the writer numbered `slot`, when both are 32-bit numbers.
    fn between(slot: u32, start: usize, end: usize) -> Option<Self> {
        let start_at = u32::try_from(start).ok()?;
        let len = u32::try_from(end - start).ok()?;

        Some(Self { slot, start: start_at, len })
    }
}

impl Payload {
    /// The writer whose arena holds the bytes.
    #[must_use]
    pub const fn slot(self) -> usize {
        let slot = match self {
            Self::InArena(span) => span.slot,
            Self::Spilled { slot, .. } => slot,
        };

        slot as usize
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
    fn mail_past_the_slab_is_spilled_and_the_slab_stays_its_size() {
        // Catches mail that runs past the slab cut short or lost, a slab left grown by it, mail after a full slab
        // written into it anyway, spilled mail read as another piece, and spilled mail kept past a clear.
        let mut arena = Arena::new(3);
        let first = put(&mut arena, &vec![1; ARENA_BYTES - 8]);
        assert!(matches!(first, Payload::InArena(_)), "the first piece of mail fits");
        assert_eq!(arena.read(first), Some(&vec![1; ARENA_BYTES - 8][..]));

        let past = put(&mut arena, &[2; 16]);
        assert!(matches!(past, Payload::Spilled { .. }), "mail past the slab is spilled");
        assert_eq!(arena.slab.capacity(), ARENA_BYTES);
        assert_eq!(arena.read(first), Some(&vec![1; ARENA_BYTES - 8][..]));

        let last = put(&mut arena, &[3; 8]);
        assert!(matches!(last, Payload::InArena(_)), "what is left of the slab holds mail that fits it");
        assert_eq!(arena.read(last), Some(&[3; 8][..]));
        let full = put(&mut arena, &[4]);
        assert_eq!(arena.read(past), Some(&[2; 16][..]));
        assert_eq!(arena.read(full), Some(&[4][..]));

        arena.clear(0);
        assert_eq!(arena.read(full), None);
    }
}
