//! A position in the text.

use super::super::{JsonError, JsonFault, TextFault};

/// A position in the text, working on bytes.
pub(super) struct Cursor<'json> {
    text: &'json str,
    offset: usize,
}

impl<'json> Cursor<'json> {
    /// At the start of `text`.
    pub(super) fn new(text: &'json str) -> Self {
        Self { text, offset: 0 }
    }

    /// The current offset in bytes.
    pub(super) fn offset(&self) -> usize {
        self.offset
    }

    /// The byte at the cursor, if any.
    pub(super) fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.offset).copied()
    }

    /// Moves past one byte.
    pub(super) fn advance(&mut self) {
        self.offset += 1;
    }

    /// Moves past spaces, tabs, line feeds, and carriage returns.
    pub(super) fn skip_whitespace(&mut self) {
        loop {
            let next = self.peek();
            let blank = matches!(next, Some(b' ' | b'\t' | b'\n' | b'\r'));
            if !blank {
                break;
            }
            self.advance();
        }
    }

    /// The text from `start` to here.
    pub(super) fn slice(&self, start: usize) -> &'json str {
        self.text.get(start..self.offset).unwrap_or_default()
    }

    /// A text refusal at this offset.
    pub(super) fn fault(&self, fault: TextFault) -> JsonError {
        JsonFault::Text { offset: self.offset, fault }.into()
    }
}
