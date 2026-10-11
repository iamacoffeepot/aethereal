//! A map key: its leaf shape, its text, its bytes, its order.

use super::layout::At;
use super::{JsonError, JsonFault, hex, integer};
use crate::plain::{read_count, take_bytes, write_count};
use crate::{DecodeError, Integer, SchemaFault, SchemaNode};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::str::from_utf8;

/// The shape a map key comes down to, through any structs of one field (ADR-0002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeyShape {
    /// An integer of any width.
    Integer(Integer),
    /// A string.
    String,
    /// Bytes with a count.
    Bytes,
    /// An array of bytes with no count.
    ByteArray(u32),
}

/// A key's place in ADR-0002's order. All keys of one map are the same case.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum KeyOrder {
    /// An integer key by numeric value.
    Number(i128),
    /// A string, bytes, or byte-array key by its bytes.
    Bytes(Vec<u8>),
}

/// A key as both sides hold it: the member name in JSON, its bytes, and its order.
pub(super) struct MapKey {
    /// The member name in JSON.
    pub(super) name: String,
    /// The key as ADR-0002 lays it out.
    pub(super) bytes: Vec<u8>,
    /// The key's place in ADR-0002's order.
    pub(super) order: KeyOrder,
}

/// The key shape at `at`, following structs of one field by loop.
pub(super) fn shape(mut at: At<'_>) -> Result<KeyShape, JsonError> {
    loop {
        match at.node() {
            SchemaNode::Integer(kind) => {
                let kind_value = *kind;
                return Ok(KeyShape::Integer(kind_value));
            }
            SchemaNode::String => {
                return Ok(KeyShape::String);
            }
            SchemaNode::Bytes => {
                return Ok(KeyShape::Bytes);
            }
            SchemaNode::Array { element, length } => {
                let element_index = *element;
                let length_value = *length;
                let element_at = at.child(element_index)?;
                let expected = SchemaNode::Integer(Integer::U8);
                let is_byte = element_at.node() == &expected;
                if !is_byte {
                    let fault = SchemaFault::KeyShape;
                    return Err(DecodeError::InvalidSchema { node: 0, fault }.into());
                }
                return Ok(KeyShape::ByteArray(length_value));
            }
            SchemaNode::Struct { fields, .. } => {
                let single = fields.len() == 1;
                if !single {
                    let fault = SchemaFault::KeyShape;
                    return Err(DecodeError::InvalidSchema { node: 0, fault }.into());
                }
                let Some(field) = fields.first() else {
                    let fault = SchemaFault::KeyShape;
                    return Err(DecodeError::InvalidSchema { node: 0, fault }.into());
                };
                let index = field.shape;
                let next = at.child(index)?;
                at = next;
            }
            SchemaNode::Boolean
            | SchemaNode::Float(_)
            | SchemaNode::Option(_)
            | SchemaNode::Sequence(_)
            | SchemaNode::Map { .. }
            | SchemaNode::Tuple { .. }
            | SchemaNode::Enum { .. } => {
                let fault = SchemaFault::KeyShape;
                return Err(DecodeError::InvalidSchema { node: 0, fault }.into());
            }
        }
    }
}

/// Reads one key from the front of `input`.
pub(super) fn take(shape: KeyShape, input: &mut &[u8]) -> Result<MapKey, DecodeError> {
    match shape {
        KeyShape::Integer(kind) => {
            let start = *input;
            let value = integer::take(kind, input)?;
            let remaining = input.len();
            let consumed = start.len().saturating_sub(remaining);
            let Some(raw) = start.get(..consumed) else {
                return Err(DecodeError::UnexpectedEnd);
            };
            let bytes = raw.to_vec();
            let name = value.to_string();
            let order = KeyOrder::Number(value);
            Ok(MapKey { name, bytes, order })
        }
        KeyShape::String => {
            let start = *input;
            let count = read_count(input)?;
            let length = count as usize;
            let data = take_bytes(input, length)?;
            let text = from_utf8(data).map_err(|_| DecodeError::InvalidUtf8)?;
            let remaining = input.len();
            let consumed = start.len().saturating_sub(remaining);
            let Some(raw) = start.get(..consumed) else {
                return Err(DecodeError::UnexpectedEnd);
            };
            let bytes = raw.to_vec();
            let name = String::from(text);
            let order = KeyOrder::Bytes(data.to_vec());
            Ok(MapKey { name, bytes, order })
        }
        KeyShape::Bytes => {
            let start = *input;
            let count = read_count(input)?;
            let length = count as usize;
            let data = take_bytes(input, length)?;
            let remaining = input.len();
            let consumed = start.len().saturating_sub(remaining);
            let Some(raw) = start.get(..consumed) else {
                return Err(DecodeError::UnexpectedEnd);
            };
            let bytes = raw.to_vec();
            let mut name = String::new();
            hex::write(data, &mut name);
            let order = KeyOrder::Bytes(data.to_vec());
            Ok(MapKey { name, bytes, order })
        }
        KeyShape::ByteArray(length) => {
            let count = length as usize;
            let data = take_bytes(input, count)?;
            let mut name = String::new();
            hex::write(data, &mut name);
            let order = KeyOrder::Bytes(data.to_vec());
            let bytes = data.to_vec();
            Ok(MapKey { name, bytes, order })
        }
    }
}

/// The key a member name states.
pub(super) fn from_name(shape: KeyShape, name: &str) -> Result<MapKey, JsonFault> {
    match shape {
        KeyShape::Integer(kind) => {
            let value = integer::from_digits(name)?;
            let mut bytes = Vec::new();
            integer::append(kind, value, &mut bytes)?;
            let key_name = String::from(name);
            let order = KeyOrder::Number(value);
            Ok(MapKey { name: key_name, bytes, order })
        }
        KeyShape::String => {
            let mut bytes = Vec::new();
            write_count(&mut bytes, name.len()).map_err(JsonFault::Encode)?;
            bytes.extend_from_slice(name.as_bytes());
            let key_name = String::from(name);
            let order = KeyOrder::Bytes(name.as_bytes().to_vec());
            Ok(MapKey { name: key_name, bytes, order })
        }
        KeyShape::Bytes => {
            let mut data = Vec::new();
            hex::read(name, &mut data)?;
            let mut bytes = Vec::new();
            write_count(&mut bytes, data.len()).map_err(JsonFault::Encode)?;
            bytes.extend_from_slice(&data);
            let key_name = String::from(name);
            let order = KeyOrder::Bytes(data);
            Ok(MapKey { name: key_name, bytes, order })
        }
        KeyShape::ByteArray(length) => {
            let mut data = Vec::new();
            hex::read(name, &mut data)?;
            let expected = length as usize;
            let found = data.len();
            let right_length = found == expected;
            if !right_length {
                return Err(JsonFault::WrongLength { expected, found });
            }
            let key_name = String::from(name);
            let order = KeyOrder::Bytes(data.clone());
            let bytes = data;
            Ok(MapKey { name: key_name, bytes, order })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Plain, Schema, SchemaNodes};
    use alloc::vec;

    #[derive(Plain)]
    struct PlayerId(u64);

    #[test]
    fn numbers_order_by_value() {
        // Catches ordering by text or by little-endian bytes, which put `10` before `2`.
        let mut orders = vec![KeyOrder::Number(10), KeyOrder::Number(2), KeyOrder::Number(-1)];
        orders.sort();
        assert_eq!(orders, vec![KeyOrder::Number(-1), KeyOrder::Number(2), KeyOrder::Number(10)]);
    }

    #[test]
    fn bytes_order_prefix_first() {
        // Catches a byte order that is not byte-by-byte with a prefix first.
        let mut orders =
            vec![KeyOrder::Bytes(b"b".to_vec()), KeyOrder::Bytes(b"abc".to_vec()), KeyOrder::Bytes(b"ab".to_vec())];
        orders.sort();
        assert_eq!(
            orders,
            vec![KeyOrder::Bytes(b"ab".to_vec()), KeyOrder::Bytes(b"abc".to_vec()), KeyOrder::Bytes(b"b".to_vec())]
        );
    }

    #[test]
    fn single_field_struct_is_its_field() {
        // Catches a key shape that stops at a wrapper struct.
        let flat = SchemaNodes::of(&<PlayerId as Schema>::SHAPE);
        let at = At::root(&flat);
        // The root is a struct of one `U64`; its child is the integer.
        let Some(root) = flat.node(1) else {
            panic!("player id root is a struct");
        };
        let SchemaNode::Struct { fields, .. } = root else {
            panic!("player id root is a struct");
        };
        let Some(field) = fields.first() else {
            panic!("player id has one field");
        };
        let index = field.shape;
        let is_root = at.node() == root;
        assert!(is_root);
        let _ = index;
        // Walking through the wrapper finds the integer.
        assert_eq!(shape(at), Ok(KeyShape::Integer(Integer::U64)));
    }

    #[test]
    fn bad_digit_text_is_refused() {
        // Catches a key read that leans on `parse` alone.
        assert_eq!(from_name(KeyShape::Integer(Integer::U32), "01").map(|_| ()), Err(JsonFault::NotDecimal));
        assert_eq!(from_name(KeyShape::Bytes, "0A").map(|_| ()), Err(JsonFault::HexDigit));
    }

    #[test]
    fn take_then_name_gives_same_bytes() {
        // Catches a key whose text and bytes disagree between the two directions.
        let shapes = [
            KeyShape::Integer(Integer::I32),
            KeyShape::Integer(Integer::U64),
            KeyShape::String,
            KeyShape::Bytes,
            KeyShape::ByteArray(2),
        ];
        let inputs: [&[u8]; 5] = [
            &[0xff, 0xff, 0xff, 0xff],
            &[0x2a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
            &[0x01, 0x00, 0x00, 0x00, b'a'],
            &[0x02, 0x00, 0x00, 0x00, 0xab, 0xcd],
            &[0x00, 0xff],
        ];
        for (position, shape) in shapes.iter().enumerate() {
            let Some(input) = inputs.get(position) else {
                panic!("missing key input");
            };
            let mut rest: &[u8] = input;
            let key_shape = *shape;
            let taken = take(key_shape, &mut rest);
            let is_ok = taken.is_ok();
            assert!(is_ok);
            if let Ok(taken) = taken {
                let back = from_name(key_shape, &taken.name);
                let is_back_ok = back.is_ok();
                assert!(is_back_ok);
                if let Ok(back) = back {
                    assert_eq!(back.bytes, taken.bytes);
                    assert_eq!(back.order, taken.order);
                }
            }
        }
    }
}
