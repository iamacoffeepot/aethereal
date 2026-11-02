//! `Json`: a JSON value carried as its text, as plain data (ADR-0038).

use super::{JsonError, JsonFault, reader};
use crate::plain::write_text;
use crate::{Boundary, Budget, Decode, DecodeError, Encode, EncodeError, Plain, Reader, Schema, Shape, Writer};
use alloc::string::String;

/// A JSON value, carried as its text (ADR-0038).
///
/// Its bytes are a string's, and its text is always one JSON value (RFC 8259), kept as it was given: it is checked
/// where a `Json` is made and where one is decoded, so `to_json` can write it as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Json(String);

impl Json {
    /// The value `text` states, its text kept as written; what the reader holds while checking is charged to
    /// `budget`.
    ///
    /// # Errors
    ///
    /// The reader's refusal when `text` is not one JSON value, and a decode fault when the budget is spent.
    pub fn parse(text: impl Into<String>, budget: &mut Budget) -> Result<Self, JsonError> {
        let text = text.into();
        reader::read(&text, budget)?;

        Ok(Self(text))
    }

    /// The value's JSON text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Refuses `text` that is not one JSON value with `DecodeError::InvalidJson`; what the reader holds is charged to
/// `budget`, and a spent budget is refused as such.
pub(super) fn check(text: &str, budget: &mut Budget) -> Result<(), DecodeError> {
    let Err(error) = reader::read(text, budget) else {
        return Ok(());
    };
    if let JsonFault::Decode(inner) = error.fault() {
        return Err(inner);
    }

    Err(DecodeError::InvalidJson)
}

impl Schema for Json {
    const SHAPE: Shape = Shape::Json;
}

impl<B: Boundary> Encode<B> for Json {
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        write_text(out.bytes(), &self.0)
    }
}

impl<B: Boundary> Decode<B> for Json {
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        let text = String::decode(input)?;
        check(&text, input.budget())?;

        Ok(Self(text))
    }
}

impl Plain for Json {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{from_bytes, to_bytes};
    use alloc::vec::Vec;

    /// `text`'s bytes as a string's: its count, then its UTF-8.
    fn string_bytes(text: &str) -> Vec<u8> {
        let Ok(bytes) = to_bytes(&String::from(text)) else {
            panic!("a string encodes");
        };

        bytes
    }

    #[test]
    fn text_that_is_not_one_value_is_refused_where_made() {
        // Catches a constructor that takes any text, which `to_json` would then write as invalid JSON.
        let refusals = ["", "{", "{\"a\":1,\"a\":2}", "1 2", "'a'", "nul"];
        for text in refusals {
            let mut budget = Budget::new(1024);
            let made = Json::parse(text, &mut budget);
            let is_text_fault = matches!(made.map_err(|error| error.fault()), Err(JsonFault::Text { .. }));
            assert!(is_text_fault, "the text `{text}`");
        }
    }

    #[test]
    fn text_is_kept_as_written() {
        // Catches a constructor or an encoder that rewrites the text: compacts it, reorders members, or quotes it.
        let text = " {\"b\": 1, \"a\": [true]} ";
        let mut budget = Budget::new(1024);
        let Ok(made) = Json::parse(text, &mut budget) else {
            panic!("the text is JSON");
        };
        assert_eq!(made.as_str(), text);
        assert_eq!(to_bytes(&made), Ok(string_bytes(text)));
    }

    #[test]
    fn decoding_text_that_is_not_one_value_is_refused() {
        // Catches a decoder that checks only UTF-8, as a string's does, and so builds a `Json` that is not JSON.
        let mut budget = Budget::new(1024);
        assert_eq!(from_bytes::<Json>(&string_bytes("[1,"), &mut budget), Err(DecodeError::InvalidJson));
    }

    #[test]
    fn a_spent_budget_is_refused_as_spent() {
        // Catches a check that reports a spent budget as text that is not JSON, hiding why the decode stopped.
        let bytes = string_bytes("[[[[[[[[[[]]]]]]]]]]");
        let mut budget = Budget::new(bytes.len() + 8);
        let decoded = from_bytes::<Json>(&bytes, &mut budget);
        let is_spent = matches!(decoded, Err(DecodeError::BudgetExceeded { .. }));
        assert!(is_spent, "{decoded:?}");
    }
}
