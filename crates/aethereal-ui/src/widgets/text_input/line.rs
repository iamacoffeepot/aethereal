//! A line of text being edited: its text, its caret, and the other end of its selection, each a byte offset at a
//! character's boundary; and the edits a text input makes to it.

/// A line of text, its caret, and its selection's anchor: the selection runs between the two, and is empty when they
/// meet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub caret: usize,
    pub anchor: usize,
}

/// Which way a caret moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toward {
    /// One character toward the start.
    Back,
    /// One character toward the end.
    Forward,
    /// To the start.
    Start,
    /// To the end.
    End,
}

/// Whether a caret's move takes the selection with it, its anchor left where it was, or collapses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selecting {
    Extend,
    Collapse,
}

impl Line {
    /// `text`, its caret and anchor at `caret` and `anchor`, each pulled back to a character's boundary within it.
    #[must_use]
    pub fn new(text: &str, caret: usize, anchor: usize) -> Self {
        Self { caret: boundary(text, caret), anchor: boundary(text, anchor), text: String::from(text) }
    }

    /// Where its selection runs, from its start to its end.
    #[must_use]
    pub fn selected(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    /// Types `typed` at the caret, in place of the selection.
    pub fn typed(&mut self, typed: &str) {
        let (start, end) = self.selected();
        self.text.replace_range(start..end, typed);
        self.caret = start + typed.len();
        self.anchor = self.caret;
    }

    /// Deletes the selection, or with none what lies between the caret and one step `toward` from it.
    pub fn delete(&mut self, toward: Toward) {
        let (start, end) = self.selected();
        let spans = start != end;
        let (start, end) = if spans {
            (start, end)
        } else {
            let other = self.step(self.caret, toward);
            (other.min(self.caret), other.max(self.caret))
        };
        self.text.replace_range(start..end, "");
        self.caret = start;
        self.anchor = start;
    }

    /// Moves the caret `toward`, taking the selection with it or collapsing it as `selecting` says. A collapsing move
    /// back or forward from a selection goes to the selection's end that way.
    pub fn moved(&mut self, toward: Toward, selecting: Selecting) {
        let (start, end) = self.selected();
        let collapsing = selecting == Selecting::Collapse;
        let spans = start != end;
        self.caret = match toward {
            Toward::Back if collapsing && spans => start,
            Toward::Forward if collapsing && spans => end,
            toward => self.step(self.caret, toward),
        };
        if collapsing {
            self.anchor = self.caret;
        }
    }

    /// Selects the whole text, the caret at its end.
    pub fn all(&mut self) {
        self.anchor = 0;
        self.caret = self.text.len();
    }

    /// The offset one step `toward` from `from`: a character back or forward, or to the start or the end, never past
    /// either.
    fn step(&self, from: usize, toward: Toward) -> usize {
        match toward {
            Toward::Back => self.text[..from].char_indices().next_back().map_or(0, |(at, _)| at),
            Toward::Forward => self.text[from..].chars().next().map_or(from, |next| from + next.len_utf8()),
            Toward::Start => 0,
            Toward::End => self.text.len(),
        }
    }
}

/// `offset`, pulled back to the nearest character's boundary of `text` at or before it, and no further than its end.
#[must_use]
pub fn boundary(text: &str, offset: usize) -> usize {
    let mut at = offset.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }

    at
}
