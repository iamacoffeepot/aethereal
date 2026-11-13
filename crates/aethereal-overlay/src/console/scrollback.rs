//! The console's scrollback (ADR-0053): what was printed, as rows no wider than the console, the last 2,000 of them,
//! each numbered as it was printed, so the scroll that shows them keeps each row by its number while older ones leave.

use crate::colour::Swatch;
use aethereal_data::Plain;
use std::collections::VecDeque;

/// The most rows kept: the last 2,000 (ADR-0053).
pub const MOST_ROWS: usize = 2_000;

/// A part of a row printed in one colour.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub text: String,
    pub swatch: Swatch,
}

impl Piece {
    /// `text`, printed in `swatch`.
    pub fn new(text: &str, swatch: Swatch) -> Self {
        Self { text: String::from(text), swatch }
    }
}

/// One row as it is drawn: its number, counted from the first row printed, and its pieces, left to right, no wider than
/// the console was when it was printed.
#[derive(Plain, Debug, Clone, Default, PartialEq, Eq)]
pub struct Row {
    pub number: u64,
    pub pieces: Vec<Piece>,
}

impl Row {
    /// The row's text, its colours left out.
    pub fn text(&self) -> String {
        self.pieces.iter().map(|piece| piece.text.as_str()).collect()
    }

    /// The row of `cells`, each a character and its colour, joined into a piece wherever the colour runs on.
    fn of(cells: &[(char, Swatch)]) -> Self {
        let mut pieces: Vec<Piece> = Vec::new();
        for &(character, swatch) in cells {
            match pieces.last_mut() {
                Some(piece) if piece.swatch == swatch => piece.text.push(character),
                Some(_) | None => pieces.push(Piece { text: String::from(character), swatch }),
            }
        }

        Self { number: 0, pieces }
    }
}

/// The rows printed, oldest first, and the number the next is given.
#[derive(Plain, Debug, Clone, Default, PartialEq, Eq)]
pub struct Scrollback {
    rows: VecDeque<Row>,
    next: u64,
}

impl Scrollback {
    /// Prints `text` in `swatch`: each of its lines wrapped at `width` characters.
    pub fn print(&mut self, text: &str, swatch: Swatch, width: usize) {
        for line in text.split('\n') {
            self.push(&[Piece::new(line, swatch)], width);
        }
    }

    /// Prints one line of `pieces`, wrapped at `width` characters.
    pub fn push(&mut self, pieces: &[Piece], width: usize) {
        for row in wrapped(pieces, width) {
            self.add(row);
        }
    }

    /// Adds `row` as the newest, numbered next, the oldest leaving past [`MOST_ROWS`].
    fn add(&mut self, row: Row) {
        self.rows.push_back(Row { number: self.next, ..row });
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

/// `pieces`, one line, cut into rows of at most `width` characters: at the last space past the middle of the row,
/// which is dropped, or else where the width ends. An empty line is one empty row.
pub fn wrapped(pieces: &[Piece], width: usize) -> Vec<Row> {
    let width = width.max(1);
    let cells: Vec<(char, Swatch)> =
        pieces.iter().flat_map(|piece| piece.text.chars().map(move |character| (character, piece.swatch))).collect();
    if cells.is_empty() {
        return vec![Row::default()];
    }

    let mut rows = Vec::new();
    let mut start = 0;
    while start < cells.len() {
        let rest = &cells[start..];
        let fits = rest.len() <= width;
        if fits {
            rows.push(Row::of(rest));
            break;
        }
        // The character just past the width is looked at too, as a space there ends the row cleanly.
        let space = rest[..=width].iter().rposition(|&(character, _)| character == ' ');
        let (row_end, next) = match space {
            Some(at) if at >= width / 2 => (at, at + 1),
            Some(_) | None => (width, width),
        };
        rows.push(Row::of(&rest[..row_end]));
        start += next;
    }

    rows
}
