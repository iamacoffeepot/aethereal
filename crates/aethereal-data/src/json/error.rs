//! The refusals of the JSON walks (ADR-0007).

use crate::{DecodeError, EncodeError};
use alloc::string::String;
use alloc::vec::Vec;
use core::{error::Error, fmt};

/// Why JSON text or its value is refused (ADR-0007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    path: JsonPath,
    fault: JsonFault,
}

impl JsonError {
    /// The steps from the root to the place the refusal concerns.
    #[must_use]
    pub fn path(&self) -> &JsonPath {
        &self.path
    }

    /// What was wrong at the path.
    #[must_use]
    pub fn fault(&self) -> JsonFault {
        self.fault
    }

    /// The same refusal, one step further out: `step` goes in front of the path.
    pub(in crate::json) fn within(mut self, step: JsonStep) -> Self {
        self.path.steps.insert(0, step);
        self
    }
}

impl From<JsonFault> for JsonError {
    fn from(fault: JsonFault) -> Self {
        let path = JsonPath { steps: Vec::new() };
        Self { path, fault }
    }
}

impl From<DecodeError> for JsonError {
    fn from(error: DecodeError) -> Self {
        let fault = JsonFault::Decode(error);
        Self::from(fault)
    }
}

impl From<EncodeError> for JsonError {
    fn from(error: EncodeError) -> Self {
        let fault = JsonFault::Encode(error);
        Self::from(fault)
    }
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let root = self.path.is_root();
        if root {
            let fault = self.fault;
            write!(f, "json: {fault}")
        } else {
            let path = &self.path;
            let fault = self.fault;
            write!(f, "json: at {path}: {fault}")
        }
    }
}

impl Error for JsonError {}

/// The steps from the root of a value to a place in it; empty at the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonPath {
    steps: Vec<JsonStep>,
}

impl JsonPath {
    /// The steps from the root, nearest the root first.
    #[must_use]
    pub fn steps(&self) -> &[JsonStep] {
        &self.steps
    }

    /// Whether the path is at the root, with no steps.
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.steps.is_empty()
    }
}

impl fmt::Display for JsonPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (position, step) in self.steps.iter().enumerate() {
            let first = position == 0;
            match step {
                JsonStep::Field(name) | JsonStep::Variant(name) => {
                    if !first {
                        f.write_str(".")?;
                    }
                    f.write_str(name)?;
                }
                JsonStep::Index(index) => {
                    write!(f, "[{index}]")?;
                }
                JsonStep::Key(name) => {
                    write!(f, "[\"{name}\"]")?;
                }
            }
        }

        Ok(())
    }
}

/// One step from the root of a value to a place in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonStep {
    /// A named field of a struct or variant.
    Field(String),
    /// An element of a sequence, array, or tuple, or one of several positional fields.
    Index(usize),
    /// A map member, by its key as written in JSON.
    Key(String),
    /// The fields of an enum's variant.
    Variant(String),
}

/// What was wrong at a path in a JSON value (ADR-0007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonFault {
    /// The text is not JSON (RFC 8259), repeats a member name, or goes on after the value.
    Text { offset: usize, fault: TextFault },
    /// The JSON value is not the form the shape is written in.
    Expected(JsonForm),
    /// A number with a fraction or an exponent where an integer of 8, 16, or 32 bits is read.
    NotAnInteger,
    /// An integer outside its width, or a number too large for its float.
    OutOfRange,
    /// A string that is not a decimal integer as the writer writes it.
    NotDecimal,
    /// A hexadecimal string with an odd number of digits.
    OddHexLength,
    /// A character in a hexadecimal string that is not `0`-`9` or `a`-`f`.
    HexDigit,
    /// An array, tuple, byte array, or wrapped option with the wrong number of elements.
    WrongLength { expected: usize, found: usize },
    /// The field the path ends at is absent and its shape is not an option.
    MissingField,
    /// The member the path ends at names no field.
    UnknownField,
    /// The name the path ends at names no variant.
    UnknownVariant,
    /// The bytes hold a selector that names no variant.
    UnknownSelector(u32),
    /// The schema nests deeper than this module walks.
    SchemaTooDeep,
    /// A refusal the byte codec already names: bytes that end early, a bad marker, a spent budget.
    Decode(DecodeError),
    /// A collection with more elements than a count can state.
    Encode(EncodeError),
}

impl fmt::Display for JsonFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text { offset, fault } => {
                write!(f, "text at offset {offset}: {fault}")
            }
            Self::Expected(form) => {
                write!(f, "expected {form}")
            }
            Self::NotAnInteger => f.write_str("not an integer"),
            Self::OutOfRange => f.write_str("out of range"),
            Self::NotDecimal => f.write_str("not decimal digits"),
            Self::OddHexLength => f.write_str("odd hexadecimal length"),
            Self::HexDigit => f.write_str("bad hexadecimal digit"),
            Self::WrongLength { expected, found } => {
                write!(f, "wrong length: expected {expected}, found {found}")
            }
            Self::MissingField => f.write_str("missing field"),
            Self::UnknownField => f.write_str("unknown field"),
            Self::UnknownVariant => f.write_str("unknown variant"),
            Self::UnknownSelector(selector) => {
                write!(f, "unknown selector {selector}")
            }
            Self::SchemaTooDeep => f.write_str("schema too deep"),
            Self::Decode(inner) => write!(f, "{inner}"),
            Self::Encode(inner) => write!(f, "{inner}"),
        }
    }
}

/// Why JSON text itself is refused, before it is any value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFault {
    /// The text ends in the middle of a value.
    UnexpectedEnd,
    /// A character starts no JSON value.
    UnexpectedCharacter,
    /// A character below `U+0020` inside a string, unescaped.
    ControlCharacter,
    /// A backslash followed by anything RFC 8259 does not list, or `\u` without four hexadecimal digits.
    InvalidEscape,
    /// A `\u` surrogate that is not the first half of a pair followed by its second half.
    LoneSurrogate,
    /// One object states a member name twice.
    RepeatedName,
    /// Text other than whitespace after the value.
    TrailingText,
}

impl fmt::Display for TextFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEnd => f.write_str("unexpected end"),
            Self::UnexpectedCharacter => f.write_str("unexpected character"),
            Self::ControlCharacter => f.write_str("control character in string"),
            Self::InvalidEscape => f.write_str("invalid escape"),
            Self::LoneSurrogate => f.write_str("lone surrogate"),
            Self::RepeatedName => f.write_str("repeated member name"),
            Self::TrailingText => f.write_str("trailing text"),
        }
    }
}

/// What a shape is written as, for `JsonFault::Expected`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonForm {
    /// A JSON boolean.
    Boolean,
    /// A JSON number (an integer of 8, 16, or 32 bits).
    Number,
    /// A string of decimal digits (an integer of 64 bits).
    Digits,
    /// A number, or the string `"NaN"`, `"Infinity"`, or `"-Infinity"`.
    Float,
    /// A JSON string.
    String,
    /// A string of lowercase hexadecimal.
    Hex,
    /// A JSON array.
    Array,
    /// A JSON object.
    Object,
    /// A variant: its name as a string, or an object with one member.
    Variant,
}

impl fmt::Display for JsonForm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Boolean => f.write_str("boolean"),
            Self::Number => f.write_str("number"),
            Self::Digits => f.write_str("digits"),
            Self::Float => f.write_str("float"),
            Self::String => f.write_str("string"),
            Self::Hex => f.write_str("hexadecimal string"),
            Self::Array => f.write_str("array"),
            Self::Object => f.write_str("object"),
            Self::Variant => f.write_str("variant"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    #[test]
    fn error_display_at_root_and_with_path() {
        // Catches a `Display` that prints the path at the root or drops it elsewhere.
        let root = JsonError::from(JsonFault::MissingField);
        assert_eq!(root.to_string(), "json: missing field");
        let nested = root.within(JsonStep::Field(String::from("id")));
        assert_eq!(nested.to_string(), "json: at id: missing field");
    }

    #[test]
    fn path_display_covers_all_steps() {
        // Catches a `Display` that dots, brackets, or quotes a step the wrong way.
        let path = JsonPath {
            steps: vec![
                JsonStep::Field(String::from("players")),
                JsonStep::Index(3),
                JsonStep::Variant(String::from("Box")),
                JsonStep::Key(String::from("w")),
            ],
        };
        assert_eq!(path.to_string(), "players[3].Box[\"w\"]");
        let root = JsonPath { steps: Vec::new() };
        assert_eq!(root.to_string(), "");
        assert!(root.is_root());
    }
}
