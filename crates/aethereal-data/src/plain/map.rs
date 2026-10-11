//! `BTreeMap`, `BTreeSet`, and under `std` `HashMap`, `HashSet` (ADR-0004).

use crate::plain::{charge_count, read_count, write_count};
use crate::{Boundary, Decode, DecodeError, Encode, EncodeError, Key, Plain, Reader, Schema, Shape, Writer};
use alloc::collections::{BTreeMap, BTreeSet};

/// Whether `next` comes strictly after the previous key, starting any sequence (ADR-0002).
fn follows<T: Ord>(last: Option<&T>, next: &T) -> bool {
    last.is_none_or(|prior| next > prior)
}

impl<K: Key, V: Schema> Schema for BTreeMap<K, V> {
    const SHAPE: Shape = Shape::Map { key: &K::SHAPE, value: &V::SHAPE };
}

impl<B: Boundary, K: Encode<B>, V: Encode<B>> Encode<B> for BTreeMap<K, V> {
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        write_count(out.bytes(), self.len())?;
        for (key, value) in self {
            key.encode(out)?;
            value.encode(out)?;
        }
        Ok(())
    }
}

impl<B: Boundary, K: Decode<B> + Ord, V: Decode<B>> Decode<B> for BTreeMap<K, V> {
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        let count = read_count(input.input())?;
        charge_count::<K>(input.budget(), count)?;
        charge_count::<V>(input.budget(), count)?;
        let length = count as usize;
        let mut map = Self::new();
        for _ in 0..length {
            let key = K::decode(input)?;
            let value = V::decode(input)?;
            let last = map.last_key_value().map(|pair| pair.0);
            let in_order = follows(last, &key);
            if in_order {
                map.insert(key, value);
            } else {
                return Err(DecodeError::KeyOrder);
            }
        }
        Ok(map)
    }
}

impl<K: Key, V: Plain> Plain for BTreeMap<K, V> {}

impl<T: Key> Schema for BTreeSet<T> {
    const SHAPE: Shape = Shape::Sequence(&T::SHAPE);
}

impl<B: Boundary, T: Encode<B>> Encode<B> for BTreeSet<T> {
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        write_count(out.bytes(), self.len())?;
        for item in self {
            item.encode(out)?;
        }
        Ok(())
    }
}

impl<B: Boundary, T: Decode<B> + Ord> Decode<B> for BTreeSet<T> {
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        let count = read_count(input.input())?;
        let length = charge_count::<T>(input.budget(), count)?;
        let mut set = Self::new();
        for _ in 0..length {
            let item = T::decode(input)?;
            let in_order = follows(set.last(), &item);
            if in_order {
                set.insert(item);
            } else {
                return Err(DecodeError::KeyOrder);
            }
        }
        Ok(set)
    }
}

impl<T: Key> Plain for BTreeSet<T> {}

#[cfg(feature = "std")]
use alloc::vec::Vec;
#[cfg(feature = "std")]
use core::hash::{BuildHasher, Hash};
#[cfg(feature = "std")]
use std::collections::{HashMap, HashSet};

#[cfg(feature = "std")]
impl<K: Key + Hash, V: Schema, S: BuildHasher> Schema for HashMap<K, V, S> {
    const SHAPE: Shape = Shape::Map { key: &K::SHAPE, value: &V::SHAPE };
}

#[cfg(feature = "std")]
impl<B: Boundary, K: Encode<B> + Ord, V: Encode<B>, S> Encode<B> for HashMap<K, V, S> {
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        write_count(out.bytes(), self.len())?;
        let mut entries: Vec<(&K, &V)> = self.iter().collect();
        entries.sort_by(|left, right| left.0.cmp(right.0));
        for (key, value) in entries {
            key.encode(out)?;
            value.encode(out)?;
        }
        Ok(())
    }
}

#[cfg(feature = "std")]
impl<B: Boundary, K: Decode<B> + Ord + Hash, V: Decode<B>, S: BuildHasher + Default> Decode<B> for HashMap<K, V, S> {
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        let count = read_count(input.input())?;
        charge_count::<K>(input.budget(), count)?;
        charge_count::<V>(input.budget(), count)?;
        let length = count as usize;
        let mut pairs: Vec<(K, V)> = Vec::new();
        for _ in 0..length {
            let key = K::decode(input)?;
            let value = V::decode(input)?;
            let last = pairs.last().map(|pair| &pair.0);
            let in_order = follows(last, &key);
            if in_order {
                pairs.push((key, value));
            } else {
                return Err(DecodeError::KeyOrder);
            }
        }
        Ok(pairs.into_iter().collect())
    }
}

#[cfg(feature = "std")]
impl<K: Key + Hash, V: Plain, S: BuildHasher + Default> Plain for HashMap<K, V, S> {}

#[cfg(feature = "std")]
impl<T: Key + Hash, S: BuildHasher> Schema for HashSet<T, S> {
    const SHAPE: Shape = Shape::Sequence(&T::SHAPE);
}

#[cfg(feature = "std")]
impl<B: Boundary, T: Encode<B> + Ord, S> Encode<B> for HashSet<T, S> {
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        write_count(out.bytes(), self.len())?;
        let mut items: Vec<&T> = self.iter().collect();
        items.sort();
        for item in items {
            item.encode(out)?;
        }
        Ok(())
    }
}

#[cfg(feature = "std")]
impl<B: Boundary, T: Decode<B> + Ord + Hash, S: BuildHasher + Default> Decode<B> for HashSet<T, S> {
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        let count = read_count(input.input())?;
        let length = charge_count::<T>(input.budget(), count)?;
        let mut items: Vec<T> = Vec::new();
        for _ in 0..length {
            let item = T::decode(input)?;
            let in_order = follows(items.last(), &item);
            if in_order {
                items.push(item);
            } else {
                return Err(DecodeError::KeyOrder);
            }
        }
        Ok(items.into_iter().collect())
    }
}

#[cfg(feature = "std")]
impl<T: Key + Hash, S: BuildHasher + Default> Plain for HashSet<T, S> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Budget, from_bytes, to_bytes};
    use alloc::string::String;

    #[test]
    fn int_keys_are_written_in_numeric_order() {
        // Catches a map codec that orders signed keys by their encoded bytes, which would write 1 before -1.
        let mut map = BTreeMap::new();
        map.insert(1i32, 1u8);
        map.insert(-1i32, 2u8);
        assert_eq!(
            to_bytes(&map),
            Ok(alloc::vec![0x02, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x02, 0x01, 0x00, 0x00, 0x00, 0x01])
        );
    }

    #[test]
    fn string_keys_sort_by_bytes_prefix_first() {
        // Catches a string order that is not byte-by-byte with a prefix first.
        let mut map = BTreeMap::new();
        map.insert(String::from("b"), 1u8);
        map.insert(String::from("ab"), 2u8);
        assert_eq!(
            to_bytes(&map),
            Ok(alloc::vec![
                0x02, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, b'a', b'b', 0x02, 0x01, 0x00, 0x00, 0x00, b'b', 0x01
            ])
        );
        let mut prefixed = BTreeMap::new();
        prefixed.insert(String::from("ab"), 2u8);
        prefixed.insert(String::from("a"), 1u8);
        assert_eq!(
            to_bytes(&prefixed),
            Ok(alloc::vec![
                0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, b'a', 0x01, 0x02, 0x00, 0x00, 0x00, b'a', b'b', 0x02
            ])
        );
    }

    #[test]
    fn unordered_map_keys_are_refused() {
        // Catches a decoder that accepts pairs out of ascending key order.
        let bytes = [0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0x02];
        assert_eq!(from_bytes::<BTreeMap<i32, u8>>(&bytes, &mut Budget::new(64)), Err(DecodeError::KeyOrder));
    }

    #[test]
    fn repeated_map_keys_are_refused() {
        // Catches a decoder that lets a later pair silently replace an earlier key.
        let bytes = [0x02, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01];
        assert_eq!(from_bytes::<BTreeMap<u8, u8>>(&bytes, &mut Budget::new(64)), Err(DecodeError::KeyOrder));
    }

    #[test]
    fn repeated_set_elements_are_refused() {
        // Catches a decoder that drops a repeated set element instead of refusing.
        let bytes = [0x02, 0x00, 0x00, 0x00, 0x01, 0x01];
        assert_eq!(from_bytes::<BTreeSet<u8>>(&bytes, &mut Budget::new(64)), Err(DecodeError::KeyOrder));
    }

    #[cfg(feature = "std")]
    #[test]
    fn hash_and_tree_maps_share_bytes() {
        // Catches a hash map that writes in hash order instead of ascending key order.
        let mut tree = BTreeMap::new();
        tree.insert(2u8, 20u8);
        tree.insert(1u8, 10u8);
        let mut hash = HashMap::new();
        hash.insert(2u8, 20u8);
        hash.insert(1u8, 10u8);
        assert_eq!(to_bytes(&hash), to_bytes(&tree));
    }

    #[cfg(feature = "std")]
    #[test]
    fn hash_and_tree_sets_share_bytes() {
        // Catches a hash set that writes in hash order instead of ascending order.
        let mut tree = BTreeSet::new();
        tree.insert(2u8);
        tree.insert(1u8);
        let mut hash = HashSet::new();
        hash.insert(2u8);
        hash.insert(1u8);
        assert_eq!(to_bytes(&hash), to_bytes(&tree));
    }
}
