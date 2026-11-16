//! The console's scrollback (ADR-0053): what was printed, as rows no wider than the console, the last 2,000 of them,
//! each numbered as it was printed, so the scroll that shows them keeps each row by its number while older ones leave.

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

/// One row as it is drawn: its number, counted from the first row printed, its pieces, left to right, no wider than the
/// console was when it was printed, and what it is drawn on.
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

    /// The row of `cells`, each a character and its ink, joined into a piece wherever the ink runs on, drawn on
    /// `shade`.
    fn of(cells: &[(char, Ink)], shade: Shade) -> Self {
        let mut pieces: Vec<Piece> = Vec::new();
        for &(character, ink) in cells {
            match pieces.last_mut() {
                Some(piece) if piece.ink == ink => piece.text.push(character),
                Some(_) | None => pieces.push(Piece { text: String::from(character), ink }),
            }
        }

        Self { number: 0, pieces, shade }
    }
}

/// The rows printed, oldest first, and the number the next is given.
#[derive(Plain, Debug, Clone, Default, PartialEq, Eq)]
pub struct Scrollback {
    rows: VecDeque<Row>,
    next: u64,
}

impl Scrollback {
    /// Prints `text` in `ink` on the sheet: each of its lines wrapped at `width` characters.
    pub fn print(&mut self, text: &str, ink: Ink, width: usize) {
        for line in text.split('\n') {
            self.push(&[Piece::new(line, ink)], Shade::Sheet, width);
        }
    }

    /// Prints one line of `pieces` on `shade`, wrapped at `width` characters.
    pub fn push(&mut self, pieces: &[Piece], shade: Shade, width: usize) {
        for row in wrapped(pieces, shade, width) {
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

/// `pieces`, one line on `shade`, cut into rows of at most `width` characters: at the last space past the middle of
/// the row, which is dropped, or else where the width ends. An empty line is one empty row.
pub fn wrapped(pieces: &[Piece], shade: Shade, width: usize) -> Vec<Row> {
    let width = width.max(1);
    let cells: Vec<(char, Ink)> =
        pieces.iter().flat_map(|piece| piece.text.chars().map(move |character| (character, piece.ink))).collect();
    if cells.is_empty() {
        return vec![Row { shade, ..Row::default() }];
    }

    let mut rows = Vec::new();
    let mut start = 0;
    while start < cells.len() {
        let rest = &cells[start..];
        let fits = rest.len() <= width;
        if fits {
            rows.push(Row::of(rest, shade));
            break;
        }
        // The character just past the width is looked at too, as a space there ends the row cleanly.
        let space = rest[..=width].iter().rposition(|&(character, _)| character == ' ');
        let (row_end, next) = match space {
            Some(at) if at >= width / 2 => (at, at + 1),
            Some(_) | None => (width, width),
        };
        rows.push(Row::of(&rest[..row_end], shade));
        start += next;
    }

    rows
}
