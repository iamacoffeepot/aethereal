//! How a widget's text is read (ADR-0056: Copy and paste): whether its reader selects it, what a scroll selects of what
//! it holds, and what part of a widget's text its screen's selection holds as it is drawn.
//!
//! The selection is its screen's, not a widget's: an anchor and a caret, each the key path of a text and a byte offset
//! into it, kept in the screen's table beside its focus. A widget that reads says so, hands its text, finds the
//! boundary nearest a place in it, and draws the part it is told is selected.

use aethereal_render::text::Colour;

/// How a widget's text is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reads {
    /// It has no text to read.
    Nothing,
    /// Its text is shown, and selected only within a scroll that selects every text ([`Selects::Texts`]): a `Text` or
    /// a `Rich`.
    Shown,
    /// Its text is selected wherever it stands, and a press on it starts a selection: a `Selectable`.
    Selectable,
}

/// What a scroll's reader selects of what it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Selects {
    /// The texts that are selectable wherever they stand ([`Reads::Selectable`]), as a console's rows are.
    #[default]
    Selectables,
    /// Every text, a `Text` and a `Rich` too, and a press on a panel within it starts a selection at the text under the
    /// pointer, as a transcript's replies are.
    Texts,
}

impl Selects {
    /// Whether a text read as `reads` is selected within a scroll that selects as this says.
    #[must_use]
    pub const fn covers(self, reads: Reads) -> bool {
        matches!((self, reads), (_, Reads::Selectable) | (Self::Texts, Reads::Shown))
    }
}

/// What part of a widget's text its screen's selection holds as it is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Selected {
    /// None.
    Nothing,
    /// The bytes from `start` to `end` of its read text, `end` past its length for the rest of it, drawn beneath it in
    /// `colour`.
    Part { start: u32, end: u32, colour: Colour },
}
