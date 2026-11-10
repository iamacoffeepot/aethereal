//! The console's scrollback (ADR-0053): what was printed, as rows no wider than the console, the last 2,000 of them,
//! and how far the view is scrolled up from the newest.

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

/// One row as it is drawn: its pieces, left to right, no wider than the console was when it was printed.
#[derive(Plain, Debug, Clone, Default, PartialEq, Eq)]
pub struct Row {
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

        Self { pieces }
    }
}

/// The rows printed, oldest first, and how many rows up from the newest the view stands.
#[derive(Plain, Debug, Clone, Default, PartialEq, Eq)]
pub struct Scrollback {
    rows: VecDeque<Row>,
    up: u32,
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

    /// Adds `row` as the newest, the oldest leaving past [`MOST_ROWS`]. A view scrolled up stays on the rows it shows.
    fn add(&mut self, row: Row) {
        self.rows.push_back(row);
        while self.rows.len() > MOST_ROWS {
            self.rows.pop_front();
        }
        let scrolled = self.up > 0;
        if scrolled {
            self.set_up((self.up() + 1).min(self.rows.len()));
        }
    }

    /// Lets go of every row.
    pub fn clear(&mut self) {
        self.rows.clear();
        self.up = 0;
    }

    /// Scrolls the view of `shown` rows up by a page, a row short of it so one row stays in view, as far as the
    /// oldest row.
    pub fn page_up(&mut self, shown: usize) {
        let page = shown.saturating_sub(1).max(1);
        let furthest = self.rows.len().saturating_sub(shown);
        self.set_up((self.up() + page).min(furthest));
    }

    /// Scrolls the view of `shown` rows down by a page, as far as the newest row.
    pub fn page_down(&mut self, shown: usize) {
        let page = shown.saturating_sub(1).max(1);
        self.set_up(self.up().saturating_sub(page));
    }

    /// Scrolls the view of `shown` rows down by `down_rows`, up when negative, held between the oldest row and the
    /// newest, and answers whether it was held: the view stopped at an end.
    pub fn scroll(&mut self, down_rows: i64, shown: usize) -> bool {
        let furthest = i64::try_from(self.rows.len().saturating_sub(shown)).unwrap_or(i64::MAX);
        let wanted = i64::try_from(self.up()).unwrap_or(i64::MAX).saturating_sub(down_rows);
        let up = wanted.clamp(0, furthest);
        self.set_up(usize::try_from(up).unwrap_or(0));

        up != wanted
    }

    /// Scrolls the view to the newest row.
    pub const fn scroll_to_newest(&mut self) {
        self.up = 0;
    }

    /// The rows in a view of `shown` rows, oldest first: the newest `shown`, or those above them as far as the view
    /// is scrolled up.
    pub fn visible(&self, shown: usize) -> impl Iterator<Item = &Row> {
        let end = self.rows.len().saturating_sub(self.up());
        let start = end.saturating_sub(shown);

        self.rows.range(start..end)
    }

    /// How many rows up from the newest the view stands.
    pub const fn up(&self) -> usize {
        self.up as usize
    }

    /// Puts the view `up` rows up from the newest: at most the rows kept, which a u32 counts.
    fn set_up(&mut self, up: usize) {
        self.up = u32::try_from(up).unwrap_or(u32::MAX);
    }

    /// How many rows are kept.
    pub fn len(&self) -> usize {
        self.rows.len()
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
