//! Plain data: values that are only their bytes (ADR-0004).

mod map;
mod scalar;
mod sequence;
mod sum;
mod text;
mod time;
mod tuple;

use crate::{Bare, BareReader, Budget, Decode, DecodeError, Encode, EncodeError, Reader, Schema};
use alloc::vec::Vec;

/// Data that is only its bytes (ADR-0004).
///
/// It needs nothing from wherever it is encoded or decoded, so it crosses
/// `Bare`, the boundary that offers nothing. The bytes are those of ADR-0002
/// for the type's shape (ADR-0003). The trait adds no method: it is the name
/// by which other code requires data that is only its bytes.
pub trait Plain: Schema + Encode<Bare> + Decode<Bare> {}

/// Encodes one value into a fresh buffer (ADR-0004).
pub fn to_bytes<T: Plain>(value: &T) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::new();
    value.encode(&mut out)?;
    Ok(out)
}

/// Decodes one value and refuses input with bytes left over (ADR-0002).
pub fn from_bytes<T: Plain>(bytes: &[u8], budget: &mut Budget) -> Result<T, DecodeError> {
    let mut reader = BareReader::new(bytes, budget);
    let value = T::decode(&mut reader)?;
    let trailing = reader.input().len();
    let clean = trailing == 0;
    if clean {
        Ok(value)
    } else {
        Err(DecodeError::TrailingBytes { count: trailing })
    }
}

/// Writes a count as an unsigned 32-bit integer, little-endian (ADR-0002).
pub fn write_count(out: &mut Vec<u8>, count: usize) -> Result<(), EncodeError> {
    let narrow = u32::try_from(count).map_err(|_| EncodeError::CountOverflow { count })?;
    out.extend_from_slice(&narrow.to_le_bytes());
    Ok(())
}

/// Writes text as a count, then that many bytes of UTF-8 (ADR-0002).
pub fn write_text(out: &mut Vec<u8>, text: &str) -> Result<(), EncodeError> {
    write_count(out, text.len())?;
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

/// Reads an unsigned 32-bit count, little-endian (ADR-0002).
pub fn read_count(input: &mut &[u8]) -> Result<u32, DecodeError> {
    read_u32(input)
}

/// Reads an unsigned 32-bit selector, little-endian (ADR-0002).
pub fn read_selector(input: &mut &[u8]) -> Result<u32, DecodeError> {
    read_u32(input)
}

/// Writes a selector as an unsigned 32-bit integer, little-endian (ADR-0002).
pub fn write_selector(out: &mut Vec<u8>, selector: u32) {
    out.extend_from_slice(&selector.to_le_bytes());
}

/// Takes `count` bytes off the front of `input` (ADR-0002).
pub fn take_bytes<'input>(input: &mut &'input [u8], count: usize) -> Result<&'input [u8], DecodeError> {
    let has_enough = input.len() >= count;
    if has_enough {
        let (head, tail) = input.split_at(count);
        *input = tail;
        Ok(head)
    } else {
        Err(DecodeError::UnexpectedEnd)
    }
}

/// Reads a fixed number of bytes off the front of `input` (ADR-0002).
pub fn read_array<const LENGTH: usize>(input: &mut &[u8]) -> Result<[u8; LENGTH], DecodeError> {
    let bytes = take_bytes(input, LENGTH)?;
    <[u8; LENGTH]>::try_from(bytes).map_err(|_| DecodeError::UnexpectedEnd)
}

/// Charges the budget for `count` elements of `T` before allocating (ADR-0002).
pub fn charge_count<T>(budget: &mut Budget, count: u32) -> Result<usize, DecodeError> {
    let each = size_of::<T>().max(1);
    let count_value = count as usize;
    let total = count_value.checked_mul(each);
    if let Some(bytes) = total {
        budget.charge(bytes)?;
        Ok(count_value)
    } else {
        let remaining = budget.remaining();
        Err(DecodeError::BudgetExceeded { requested: usize::MAX, remaining })
    }
}

fn read_u32(input: &mut &[u8]) -> Result<u32, DecodeError> {
    Ok(u32::from_le_bytes(read_array(input)?))
}
