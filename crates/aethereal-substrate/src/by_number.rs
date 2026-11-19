//! A map under numbers the engine counts out itself, such as an actor's counts of the mail it sends, which are no
//! hash of anything and so take a hash of their own.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// A map under counted numbers, or under ids paired with them.
///
/// A lookup is one probe into one table, however many it holds. Its hash is the same on every run, so the order a map
/// is walked in follows only from what was put in it and taken out, in the order it was.
pub type ByNumber<K, V> = HashMap<K, V, BuildHasherDefault<NumberBits>>;

/// The hash of counted numbers: each word mixed in by a multiply, and its high half folded into its low half, so
/// numbers a step apart, whose low bits alone differ, fall far apart in both the place a table looks in and the tag
/// it compares.
#[derive(Default)]
pub struct NumberBits(u64);

/// An odd constant with its bits spread evenly: 2^64 over the golden ratio.
const SPREAD: u64 = 0x9e37_79b9_7f4a_7c15;

impl Hasher for NumberBits {
    fn finish(&self) -> u64 {
        self.0 ^ (self.0 >> 32)
    }

    /// For a key written as bytes; a number is written whole, by [`Self::write_u64`].
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.write_u64(u64::from(*byte));
        }
    }

    fn write_u64(&mut self, bits: u64) {
        self.0 = (self.0.rotate_left(5) ^ bits).wrapping_mul(SPREAD);
    }
}
