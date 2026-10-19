//! Where the bytes of the mail a turn sends are written: an arena of the worker that ran it (ADR-0011).

#[cfg(feature = "std")]
use crate::{Actors, ActorsWriter};
#[cfg(feature = "std")]
use aethereal_data::{Encode, EncodeError, Writer};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;
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
    pieces: Pieces,
    /// The pins the mail in the slab holds, each at where its reference begins in the slab, in that order: dropped
    /// when the arena is emptied (ADR-0014).
    #[cfg(feature = "std")]
    held: Vec<crate::Held>,
    /// The pins the spilled mail holds, each with the index of its piece and where in that piece its reference
    /// begins, in that order.
    #[cfg(feature = "std")]
    held_spilled: Vec<(u32, crate::Held)>,
}

/// The pieces of mail an arena holds, apart from the pins they hold, so that a write keeps pins while it writes them.
#[derive(Debug)]
struct Pieces {
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

/// The slot of no writer: a payload with it lies in the bytes its receiver's part carried over from a step in which
/// the receiver was not handed it.
pub const CARRIED: u32 = u32::MAX;

/// Where the bytes of one piece of mail are, in the arena of the writer that wrote them, or of a run of mail: sixteen
/// bytes, carried with every piece of mail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Payload {
    /// In the writer's slab: `len` bytes from `start`, the writer numbered `slot`'s. They are `pieces` pieces of
    /// mail of `len / pieces` bytes each, one after another: one, but for a run, what one turn sent to one receiver,
    /// of one kind, back to back, by sends it named. The count of pieces sits beside the variant's tag, so a payload stays sixteen
    /// bytes, and a run holds at most `u16::MAX` pieces.
    InArena { slot: u32, start: u32, len: u32, pieces: u16 },
    /// Spilled, as it did not fit: the writer's `index`th spilled piece.
    Spilled { slot: u32, index: u32 },
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

        Self {
            pieces: Pieces { slot, slab: Vec::new(), spilled: Vec::new(), bytes: ARENA_BYTES },
            #[cfg(feature = "std")]
            held: Vec::new(),
            #[cfg(feature = "std")]
            held_spilled: Vec::new(),
        }
    }

    /// Which writer's arena this is.
    #[must_use]
    pub const fn slot(&self) -> usize {
        self.pieces.slot as usize
    }

    /// Empties the arena, keeping its slab and freeing what was spilled, for when its writer writes it again, and
    /// gives its slab `bytes`, at least [`ARENA_BYTES`] and at most [`SLAB_MOST`]: the writer's share of the host's
    /// quota for mail.
    pub fn clear(&mut self, bytes: usize) {
        self.pieces.slab.clear();
        self.pieces.spilled.clear();
        // What was never read lets go of its blobs with it.
        #[cfg(feature = "std")]
        {
            self.held.clear();
            self.held_spilled.clear();
        }
        self.pieces.bytes = bytes.clamp(ARENA_BYTES, SLAB_MOST);
    }

    /// The bytes `payload` says where to find, when they are in this arena; nothing otherwise.
    #[must_use]
    pub fn read(&self, payload: Payload) -> Option<&[u8]> {
        let is_own = payload.slot() == self.slot();
        let bytes = match payload {
            Payload::InArena { start, len, .. } => {
                let start = start as usize;

                self.pieces.slab.get(start..start.checked_add(len as usize)?)
            }
            Payload::Spilled { index, .. } => self.pieces.spilled.get(index as usize).map(|bytes| &bytes[..]),
        };

        is_own.then_some(bytes).flatten()
    }

    /// Writes one piece of mail with `write`, which appends its bytes to the vector it is given, and answers
    /// with where they are: in the slab when they fit, spilled otherwise. Nothing is written
    /// when `write` fails.
    pub(crate) fn write<E>(&mut self, write: impl FnOnce(Vec<u8>) -> (Vec<u8>, Result<(), E>)) -> Result<Payload, E> {
        self.pieces.write(write)
    }
}

impl Pieces {
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

    /// As [`Arena::write`].
    fn write<E>(&mut self, write: impl FnOnce(Vec<u8>) -> (Vec<u8>, Result<(), E>)) -> Result<Payload, E> {
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
        if fits && let Some(payload) = Payload::between(self.slot, start, self.slab.len()) {
            return Ok(payload);
        }
        // It ran past the slab: its bytes are spilled, and the slab returns to its size.
        let own = self.slab.split_off(start);
        self.slab.shrink_to(self.bytes);

        Ok(self.spill(own))
    }
}

#[cfg(feature = "std")]
impl Arena {
    /// Writes one piece of mail with `write`, as [`Self::write`] does, which is handed beside the bytes the list to
    /// keep the pins of the blobs it refers to, each at where its reference begins. The arena keeps them with the
    /// piece, and drops them when the piece is not written.
    pub(crate) fn write_holding<E>(
        &mut self,
        write: impl FnOnce(Vec<u8>, &mut Vec<crate::Held>) -> (Vec<u8>, Result<(), E>),
    ) -> Result<Payload, E> {
        let Self { pieces, held, .. } = self;
        let mark = held.len();
        let mut begins = 0;
        let written = pieces.write(|bytes| {
            begins = bytes.len();
            write(bytes, held)
        });
        let pinned = held.len() > mark;
        if pinned {
            self.file_pins(&written, mark, begins);
        }

        written
    }

    /// Writes `mail` as one piece of mail, as [`Self::write`] does, with the pins on the blobs it refers to, and
    /// answers with where it is: for mail no turn sends, which carries no resource (ADR-0010).
    ///
    /// # Errors
    ///
    /// Why `mail` could not be written, as [`EncodeError::NotCarried`] when it holds a resource on its way.
    pub fn write_mail<K: Encode<Actors>>(&mut self, mail: &K) -> Result<Payload, EncodeError> {
        self.write_holding(|bytes, held| {
            let mut out = Writer::<Actors>::appending(bytes, ActorsWriter::new(held));
            let written = mail.encode(&mut out);

            (out.into_bytes(), written)
        })
    }

    /// Files the pins a write kept from `mark` on in [`Self::held`] by what became of the piece: kept where they are
    /// in the slab, moved with it when it spilled, and dropped when it was not written.
    #[inline(never)]
    fn file_pins<E>(&mut self, written: &Result<Payload, E>, mark: usize, begins: usize) {
        match *written {
            Ok(Payload::InArena { .. }) => {}
            Ok(Payload::Spilled { index, .. }) => {
                let begins = u32::try_from(begins).unwrap_or(u32::MAX);
                let moved = self.held.drain(mark..).map(|(at, pin)| (index, (at - begins, pin)));
                self.held_spilled.extend(moved);
            }
            Err(_) => self.held.truncate(mark),
        }
    }

    /// Adds to `into` a pin of its own on each blob the bytes of `payload` refer to, when they are in this arena, each
    /// at `base` more than where its reference begins in them: for bytes copied out to `base`.
    #[inline]
    pub fn pin_copied(&self, payload: Payload, base: u32, into: &mut Vec<crate::Held>) {
        let is_own = payload.slot() == self.slot();
        let holds = !self.held.is_empty() || !self.held_spilled.is_empty();
        if is_own && holds {
            self.pin_held(payload, base, into);
        }
    }

    /// [`Self::pin_copied`], for an arena whose mail holds pins: most holds none.
    #[inline(never)]
    fn pin_held(&self, payload: Payload, base: u32, into: &mut Vec<crate::Held>) {
        self.held_by(payload).each(|at, pin| into.push((base + at, pin.clone())));
    }

    /// Whether any of its mail holds a pin.
    #[must_use]
    pub fn holds_pins(&self) -> bool {
        !self.held.is_empty() || !self.held_spilled.is_empty()
    }

    /// Lets go of the pins its mail holds, keeping the mail: for a guest, whose host keeps pins of its own on what the
    /// mail refers to once it has read the guest's records of it (ADR-0014, ADR-0015).
    #[cfg(feature = "guest")]
    pub(crate) fn let_go(&mut self) {
        self.held.clear();
        self.held_spilled.clear();
    }

    /// Where the references in the bytes of `payload` lie, and the pins on their blobs, when they are in this arena:
    /// for a host, which hands the bytes to code that takes pins of its own (ADR-0014).
    #[must_use]
    pub fn pinned(&self, payload: Payload) -> Pinned<'_> {
        let is_own = payload.slot() == self.slot();
        let holds = !self.held.is_empty() || !self.held_spilled.is_empty();
        if !is_own || !holds {
            return Pinned::Nothing;
        }

        self.held_by(payload)
    }

    /// [`Self::pinned`], for an arena whose mail holds pins.
    fn held_by(&self, payload: Payload) -> Pinned<'_> {
        match payload {
            Payload::InArena { start, len, .. } => Pinned::Lying { held: crate::within(&self.held, start, len), start },
            Payload::Spilled { index, .. } => {
                let first = self.held_spilled.partition_point(|(piece, _)| *piece < index);
                let past = self.held_spilled.partition_point(|(piece, _)| *piece <= index);

                Pinned::Spilled { held: &self.held_spilled[first..past] }
            }
        }
    }
}

/// Where the references in some bytes of mail lie, and the pins on their blobs: what an arena, a part's carried bytes,
/// or a hop keeps beside them, shown to a host that hands the bytes to code that takes pins of its own (ADR-0014).
#[cfg(feature = "std")]
#[derive(Clone, Copy)]
pub enum Pinned<'a> {
    /// The bytes refer to no blob.
    Nothing,
    /// The pins among `held`, each at where its reference begins, counted from `start`.
    Lying { held: &'a [crate::Held], start: u32 },
    /// The pins of a spilled piece, each at where its reference begins in it.
    Spilled { held: &'a [(u32, crate::Held)] },
}

#[cfg(feature = "std")]
impl Pinned<'_> {
    /// Shows `visit` each reference: where it begins in the bytes, and the pin on its blob.
    pub fn each(self, mut visit: impl FnMut(u32, &crate::BlobRef)) {
        match self {
            Self::Nothing => {}
            Self::Lying { held, start } => {
                for (at, pin) in held {
                    visit(at - start, pin);
                }
            }
            Self::Spilled { held } => {
                for (_, (at, pin)) in held {
                    visit(*at, pin);
                }
            }
        }
    }
}

impl Arena {
    /// Writes `text`, as `Display` shows it, and answers with where it is, as [`Self::write`] does a piece of mail.
    /// Nothing is written when `Display` fails.
    ///
    /// # Errors
    ///
    /// The error `Display` returned.
    pub fn write_text(&mut self, text: impl fmt::Display) -> Result<Payload, fmt::Error> {
        self.write(|bytes| {
            let mut out = Text(bytes);
            let written = fmt::write(&mut out, format_args!("{text}"));

            (out.0, written)
        })
    }
}

/// Text, written onto the end of an arena's bytes.
struct Text(Vec<u8>);

impl fmt::Write for Text {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0.extend_from_slice(text.as_bytes());

        Ok(())
    }
}

impl Payload {
    /// The bytes from `start` to `end` of the arena of the writer numbered `slot`, one piece of mail, when both are
    /// 32-bit numbers.
    fn between(slot: u32, start: usize, end: usize) -> Option<Self> {
        let start_at = u32::try_from(start).ok()?;
        let len = u32::try_from(end - start).ok()?;

        Some(Self::InArena { slot, start: start_at, len, pieces: 1 })
    }

    /// The writer whose arena holds the bytes.
    #[must_use]
    pub const fn slot(self) -> usize {
        let slot = match self {
            Self::InArena { slot, .. } | Self::Spilled { slot, .. } => slot,
        };

        slot as usize
    }

    /// How many pieces of mail it holds: one, but for a run.
    #[must_use]
    pub const fn pieces(self) -> u32 {
        match self {
            Self::InArena { pieces, .. } => pieces as u32,
            Self::Spilled { .. } => 1,
        }
    }

    /// How many bytes each of its pieces is, for a run.
    #[must_use]
    pub const fn each(self) -> u32 {
        match self {
            Self::InArena { len, pieces, .. } => len / pieces as u32,
            Self::Spilled { .. } => 0,
        }
    }

    /// The piece numbered `index` of what it holds, each of its pieces `each` bytes: itself, but for a run.
    #[must_use]
    pub const fn piece(self, index: u32, each: u32) -> Self {
        match self {
            Self::InArena { slot, start, pieces, .. } if pieces > 1 => {
                Self::InArena { slot, start: start + index * each, len: each, pieces: 1 }
            }
            Self::InArena { .. } | Self::Spilled { .. } => self,
        }
    }

    /// Each piece of mail it holds, in order, as a payload of its own: itself, but for a run.
    pub fn each_piece(self) -> impl Iterator<Item = Self> {
        let each = self.each();

        (0..self.pieces()).map(move |index| self.piece(index, each))
    }

    /// What it holds from the piece numbered `index` on, one of its pieces: itself, for the first.
    #[must_use]
    pub fn from_piece(self, index: u32) -> Self {
        match self {
            Self::InArena { slot, start, len, pieces } if index > 0 => {
                let each = len / u32::from(pieces);
                let taken = u16::try_from(index).unwrap_or(pieces).min(pieces);

                Self::InArena {
                    slot,
                    start: start + u32::from(taken) * each,
                    len: len - u32::from(taken) * each,
                    pieces: pieces - taken,
                }
            }
            Self::InArena { .. } | Self::Spilled { .. } => self,
        }
    }

    /// What a run holds once its first piece is taken out: nothing for one piece.
    #[must_use]
    pub const fn rest(self) -> Option<Self> {
        match self {
            Self::InArena { slot, start, len, pieces } if pieces > 1 => {
                let each = len / pieces as u32;

                Some(Self::InArena { slot, start: start + each, len: len - each, pieces: pieces - 1 })
            }
            Self::InArena { .. } | Self::Spilled { .. } => None,
        }
    }

    /// It, with `next` after it, as one run: when `next` is one piece lying in the slab straight after it, as long as
    /// each of its pieces, and the run has room.
    #[must_use]
    pub const fn joined(self, next: Self) -> Option<Self> {
        let Self::InArena { slot: next_slot, start: next_start, len: next_len, pieces: 1 } = next else {
            return None;
        };
        let Self::InArena { slot, start, len, pieces } = self else {
            return None;
        };
        let is_after = next_slot == slot && next_start == start + len;
        let is_alike = next_len == len / pieces as u32;
        let has_room = pieces < u16::MAX;

        if is_after && is_alike && has_room {
            Some(Self::InArena { slot, start, len: len + next_len, pieces: pieces + 1 })
        } else {
            None
        }
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
        assert!(matches!(first, Payload::InArena { .. }), "the first piece of mail fits");
        assert_eq!(arena.read(first), Some(&vec![1; ARENA_BYTES - 8][..]));

        let past = put(&mut arena, &[2; 16]);
        assert!(matches!(past, Payload::Spilled { .. }), "mail past the slab is spilled");
        assert_eq!(arena.pieces.slab.capacity(), ARENA_BYTES);
        assert_eq!(arena.read(first), Some(&vec![1; ARENA_BYTES - 8][..]));

        let last = put(&mut arena, &[3; 8]);
        assert!(matches!(last, Payload::InArena { .. }), "what is left of the slab holds mail that fits it");
        assert_eq!(arena.read(last), Some(&[3; 8][..]));
        let full = put(&mut arena, &[4]);
        assert_eq!(arena.read(past), Some(&[2; 16][..]));
        assert_eq!(arena.read(full), Some(&[4][..]));

        arena.clear(0);
        assert_eq!(arena.read(full), None);
    }
}
