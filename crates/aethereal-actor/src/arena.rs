//! Where the bytes of the mail a turn sends are written: an arena of the worker that ran it (ADR-0011).

use alloc::vec::Vec;

/// The bytes of the mail one writer sends in a step, written one after another (ADR-0011).
///
/// A piece of mail is handled in the step after the one it is sent in, or
/// dropped, so its bytes are read in that next step and never after. The
/// engine keeps two arenas for each writer, one for each parity of the step,
/// and clears an arena when the writer's turn to write it comes round again:
/// nothing in it is freed, and no reader counts what it holds.
#[derive(Debug, Default)]
pub struct Arena {
    /// Which writer's arena this is, as the engine numbers them.
    slot: usize,
    bytes: Vec<u8>,
}

/// Where the bytes of one piece of mail are: a range of one writer's arena.
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
    /// The empty arena of the writer numbered `slot`.
    #[must_use]
    pub const fn new(slot: usize) -> Self {
        Self { slot, bytes: Vec::new() }
    }

    /// Which writer's arena this is.
    #[must_use]
    pub const fn slot(&self) -> usize {
        self.slot
    }

    /// Empties the arena, keeping what it has allocated: for when its writer writes it again.
    pub fn clear(&mut self) {
        self.bytes.clear();
    }

    /// The bytes at `span`, when it is a span of this arena; nothing otherwise.
    #[must_use]
    pub fn read(&self, span: Span) -> Option<&[u8]> {
        let is_own = span.slot == self.slot;
        let end = span.start.checked_add(span.len)?;

        is_own.then(|| self.bytes.get(span.start..end)).flatten()
    }

    /// The bytes written so far, to append a piece of mail to, and where it begins.
    pub(crate) const fn bytes(&mut self) -> &mut Vec<u8> {
        &mut self.bytes
    }
}
