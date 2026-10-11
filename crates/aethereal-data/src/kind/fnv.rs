//! The one FNV-1a fold every hash in `kind` goes through (ADR-0006).

/// The FNV-1a 64-bit offset basis: the hash before any byte is folded.
const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// The FNV-1a 64-bit prime: the multiplier after each byte is xored in.
const PRIME: u64 = 0x0100_0000_01b3;

/// A running FNV-1a hash.
#[derive(Clone, Copy)]
pub(super) struct Fnv(u64);

impl Fnv {
    /// The offset basis, before any byte is folded.
    pub(super) const fn new() -> Self {
        Self(OFFSET_BASIS)
    }

    /// Each byte xored in, then multiplied by the prime, wrapping.
    pub(super) const fn bytes(self, bytes: &[u8]) -> Self {
        let mut hash = self.0;
        let mut index = 0;
        while index < bytes.len() {
            hash ^= bytes[index] as u64;
            hash = hash.wrapping_mul(PRIME);
            index += 1;
        }

        Self(hash)
    }

    /// One byte folded.
    pub(super) const fn byte(self, byte: u8) -> Self {
        self.bytes(&[byte])
    }

    /// A count, a length, a selector: four bytes little-endian.
    pub(super) const fn number(self, number: u32) -> Self {
        let bytes = number.to_le_bytes();

        self.bytes(&bytes)
    }

    /// A slice's length as a `number`; the other branch cannot be reached:
    /// a count is decoded from a `u32`, and a constant shape that large cannot be compiled.
    pub(super) const fn count(self, length: usize) -> Self {
        let fits = length <= u32::MAX as usize;
        #[allow(clippy::cast_possible_truncation, reason = "the test above proves the length fits in u32")]
        let number = if fits {
            length as u32
        } else {
            u32::MAX
        };

        self.number(number)
    }

    /// A child's layout hash: eight bytes little-endian.
    pub(super) const fn hash(self, hash: u64) -> Self {
        let bytes = hash.to_le_bytes();

        self.bytes(&bytes)
    }

    /// The finished 64-bit hash.
    pub(super) const fn finish(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_vectors_match() {
        // Catches multiply-before-xor and a wrong prime.
        // Tripwire: the published FNV-1a 64 vectors.
        assert_eq!(Fnv::new().finish(), 0xcbf2_9ce4_8422_2325);
        assert_eq!(Fnv::new().bytes(b"a").finish(), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(Fnv::new().bytes(b"foobar").finish(), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn multibyte_folds_are_little_endian() {
        // Catches a wrong width or byte order.
        assert_eq!(Fnv::new().number(0x0403_0201).finish(), Fnv::new().bytes(&[1, 2, 3, 4]).finish());
        assert_eq!(Fnv::new().count(3).finish(), Fnv::new().bytes(&[3, 0, 0, 0]).finish());
        assert_eq!(
            Fnv::new().hash(0x0807_0605_0403_0201).finish(),
            Fnv::new().bytes(&[1, 2, 3, 4, 5, 6, 7, 8]).finish()
        );
    }
}
