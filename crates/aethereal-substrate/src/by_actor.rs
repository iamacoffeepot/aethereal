//! A map under actors' ids, which needs no hash of its own (ADR-0008).

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// A map under actors' ids.
///
/// An id is already a hash of the actor's path, so it is used as its own hash, its high half folded into its low
/// half. A lookup is one probe into one table, where a tree takes a chain of loads as deep as the tree.
pub type ByActor<V> = HashMap<u64, V, BuildHasherDefault<IdBits>>;

/// The hash of an actor's id: the id's own bits.
#[derive(Default)]
pub struct IdBits(u64);

impl Hasher for IdBits {
    fn finish(&self) -> u64 {
        self.0 ^ (self.0 >> 32)
    }

    /// For a key written as bytes; an id is written whole, by [`Self::write_u64`].
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = self.0.rotate_left(8) ^ u64::from(*byte);
        }
    }

    fn write_u64(&mut self, bits: u64) {
        self.0 = bits;
    }
}
