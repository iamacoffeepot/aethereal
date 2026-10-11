//! Encode and decode failures for plain data (ADR-0004).

use crate::SchemaFault;
use core::error::Error;
use core::fmt;

/// A plain-data encode failure (ADR-0004).
///
/// Encoding fails only when a collection holds more elements than a count can state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    /// A collection holds more elements than a count can state.
    CountOverflow {
        /// The element count that did not fit in a `u32` count.
        count: usize,
    },
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CountOverflow { count } => write!(f, "plain data: collection of {count} exceeds the count ceiling"),
        }
    }
}

impl Error for EncodeError {}

/// A plain-data decode failure (ADR-0002, ADR-0004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// Input ended in the middle of a value.
    UnexpectedEnd,
    /// Input had bytes left over after the value.
    TrailingBytes {
        /// The number of bytes left over after the value.
        count: usize,
    },
    /// A boolean byte was neither `0` nor `1`.
    InvalidBoolean(u8),
    /// An option marker byte was neither `0` nor `1`.
    InvalidOption(u8),
    /// String bytes were not valid UTF-8.
    InvalidUtf8,
    /// An enum selector names no variant.
    InvalidSelector {
        /// The enum's name, for error messages.
        name: &'static str,
        /// The selector that named no variant.
        selector: u32,
    },
    /// A `char` code point was not a Unicode scalar value.
    InvalidChar(u32),
    /// A `NonZero` integer decoded from zero.
    Zero,
    /// A `Duration` held one billion nanoseconds or more.
    InvalidNanos(u32),
    /// A map key or set element that is not greater than the one before it.
    KeyOrder,
    /// A flat schema node breaks the flat form.
    InvalidSchema {
        /// The index of the offending node, or `0` when there is none.
        node: u32,
        /// Why the node is refused.
        fault: SchemaFault,
    },
    /// A decode would allocate past its budget.
    BudgetExceeded {
        /// The bytes the decode asked to allocate.
        requested: usize,
        /// The bytes the budget still held.
        remaining: usize,
    },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEnd => f.write_str("plain data: unexpected end of input"),
            Self::TrailingBytes { count } => write!(f, "plain data: {count} trailing bytes after value"),
            Self::InvalidBoolean(byte) => write!(f, "plain data: invalid boolean byte {byte}"),
            Self::InvalidOption(byte) => write!(f, "plain data: invalid option byte {byte}"),
            Self::InvalidUtf8 => f.write_str("plain data: string is not valid UTF-8"),
            Self::InvalidSelector { name, selector } => {
                write!(f, "plain data: invalid selector {selector} for enum {name}")
            }
            Self::InvalidChar(value) => write!(f, "plain data: invalid char code point {value}"),
            Self::Zero => f.write_str("plain data: zero for a non-zero integer"),
            Self::InvalidNanos(value) => write!(f, "plain data: invalid nanos {value}"),
            Self::KeyOrder => f.write_str("plain data: map keys out of order"),
            Self::InvalidSchema { node, fault } => {
                write!(f, "plain data: invalid schema at node {node}: {fault}")
            }
            Self::BudgetExceeded { requested, remaining } => {
                write!(f, "plain data: budget exceeded: requested {requested}, remaining {remaining}")
            }
        }
    }
}

impl Error for DecodeError {}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn count_overflow_names_the_count() {
        let message = EncodeError::CountOverflow { count: 7 }.to_string();
        assert_eq!(message, "plain data: collection of 7 exceeds the count ceiling");
    }

    #[test]
    fn selector_names_the_enum_and_selector() {
        let message = DecodeError::InvalidSelector { name: "Result", selector: 2 }.to_string();
        assert_eq!(message, "plain data: invalid selector 2 for enum Result");
    }

    #[test]
    fn budget_names_request_and_remainder() {
        let error = DecodeError::BudgetExceeded { requested: 9, remaining: 4 };
        let message = error.to_string();
        assert_eq!(message, "plain data: budget exceeded: requested 9, remaining 4");
    }
}
