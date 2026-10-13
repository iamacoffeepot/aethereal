//! A short reference to an actor's path (ADR-0008).

use crate::Plain;

/// A short reference to an actor's path: 64 bits that stand for it within one engine (ADR-0008).
///
/// Only the engine computes one, with the function that folds a path into it,
/// and neither a guest, a tool, nor another engine does. An id that was decoded
/// is a claim like any other plain data: it names an actor only once the engine
/// has found it in its table. An id is not an address: nothing sends to one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
pub struct ActorId(u64);

impl ActorId {
    /// The id whose 64 bits are `bits`, for the engine, which alone computes them.
    #[must_use]
    pub const fn new(bits: u64) -> Self {
        Self(bits)
    }

    /// The id's 64 bits.
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }
}
