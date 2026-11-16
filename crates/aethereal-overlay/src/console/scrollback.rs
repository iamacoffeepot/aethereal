//! The console's scrollback (ADR-0053): what was printed, a row for each line, the last 2,000 of them, each numbered as
//! it was printed, so the scroll that shows them keeps each row by its number while older ones leave. A row is never
//! cut to the console's width here: the scroll lays each out wrapped at the width it gives it, by the font's measure.

use crate::colour::Ink;
use aethereal_data::Plain;
use std::collections::VecDeque;

/// The most rows kept: the last 2,000 (ADR-0053).
pub const MOST_ROWS: usize = 2_000;

/// A part of a row printed in one ink.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub text: String,
    pub ink: Ink,
}

impl Piece {
    /// `text`, printed in `ink`.
    pub fn new(text: &str, ink: Ink) -> Self {
        Self { text: String::from(text), ink }
    }
}

/// What a row is drawn on.
#[derive(Plain, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Shade {
    /// The scrollback's own sheet.
    #[default]
    Sheet,
    /// The raised band of a line the owner entered, as a prompt's lines are set apart in a terminal.
    Raised,
}

/// One row as it is drawn: its number, counted from the first row printed, its pieces, left to right, one line of what
/// was printed, and what it is drawn on.
#[derive(Plain, Debug, Clone, Default, PartialEq, Eq)]
pub struct Row {
    pub number: u64,
    pub pieces: Vec<Piece>,
    pub shade: Shade,
}

impl Row {
    /// The row's text, its colours left out.
    pub fn text(&self) -> String {
        self.pieces.iter().map(|piece| piece.text.as_str()).collect()
    }
}

/// The rows printed, oldest first, and the number the next is given.
#[derive(Plain, Debug, Clone, Default, PartialEq, Eq)]
pub struct Scrollback {
    rows: VecDeque<Row>,
    next: u64,
}

impl Scrollback {
    /// Prints `text` in `ink` on the sheet: a row for each of its lines.
    pub fn print(&mut self, text: &str, ink: Ink) {
        for line in text.split('\n') {
            self.push(vec![Piece::new(line, ink)], Shade::Sheet);
        }
    }

    /// Prints one line of `pieces` on `shade` as the newest row, numbered next, the oldest leaving past [`MOST_ROWS`].
    pub fn push(&mut self, pieces: Vec<Piece>, shade: Shade) {
        self.rows.push_back(Row { number: self.next, pieces, shade });
        self.next += 1;
        while self.rows.len() > MOST_ROWS {
            self.rows.pop_front();
        }
    }

    /// Lets go of every row.
    pub fn clear(&mut self) {
        self.rows.clear();
    }

    /// Every row kept, oldest first.
    pub fn rows(&self) -> impl ExactSizeIterator<Item = &Row> {
        self.rows.iter()
    }
}
