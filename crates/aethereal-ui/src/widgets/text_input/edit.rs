//! A text being edited: its text, its caret, and the other end of its selection, each a byte offset at a character's
//! boundary; the edits a text input makes to it; and where a caret goes by character and by word, which no layout
//! decides.

/// A text, its caret, and its selection's anchor: the selection runs between the two, and is empty when they meet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub text: String,
    pub caret: usize,
    pub anchor: usize,
}

/// Whether a caret's move takes the selection with it, its anchor left where it was, or collapses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selecting {
    Extend,
    Collapse,
}

/// What a character is to a move by word: a word is a run of one class, and spaces and line breaks lie between words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// A letter, a digit, or `_`.
    Word,
    /// Any other mark: punctuation, a symbol.
    Mark,
    /// White space within a line.
    Space,
    /// A line break, `\n`.
    Break,
}

impl Class {
    /// The class of `character`.
    fn of(character: char) -> Self {
        match character {
            '\n' => Self::Break,
            _ if character.is_whitespace() => Self::Space,
            _ if character.is_alphanumeric() || character == '_' => Self::Word,
            _ => Self::Mark,
        }
    }

    /// Whether a move by word passes over it to reach a word.
    const fn between(self) -> bool {
        matches!(self, Self::Space | Self::Break)
    }
}

impl Edit {
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

    /// The text selected.
    #[must_use]
    pub fn selection(&self) -> &str {
        let (start, end) = self.selected();

        &self.text[start..end]
    }

    /// Whether anything is selected.
    #[must_use]
    pub fn selects(&self) -> bool {
        self.caret != self.anchor
    }

    /// Types `typed` at the caret, in place of the selection.
    pub fn typed(&mut self, typed: &str) {
        let (start, end) = self.selected();
        self.text.replace_range(start..end, typed);
        self.caret = start + typed.len();
        self.anchor = self.caret;
    }

    /// Deletes the selection, or with none what lies between the caret and `to`.
    pub fn delete(&mut self, to: usize) {
        let (start, end) = if self.selects() {
            self.selected()
        } else {
            (to.min(self.caret), to.max(self.caret))
        };
        self.text.replace_range(start..end, "");
        self.caret = start;
        self.anchor = start;
    }

    /// Moves the caret to `to`, taking the selection with it or collapsing it as `selecting` says.
    pub fn move_to(&mut self, to: usize, selecting: Selecting) {
        self.caret = boundary(&self.text, to);
        if selecting == Selecting::Collapse {
            self.anchor = self.caret;
        }
    }

    /// Selects from `from` to `to`, the caret at `to`.
    pub fn select(&mut self, from: usize, to: usize) {
        self.anchor = boundary(&self.text, from);
        self.caret = boundary(&self.text, to);
    }

    /// Selects the whole text, the caret at its end.
    pub fn all(&mut self) {
        self.select(0, self.text.len());
    }

    /// The character's boundary before `from`, or the start.
    #[must_use]
    pub fn back(&self, from: usize) -> usize {
        self.text[..from].char_indices().next_back().map_or(0, |(at, _)| at)
    }

    /// The character's boundary after `from`, or the end.
    #[must_use]
    pub fn forward(&self, from: usize) -> usize {
        self.text[from..].chars().next().map_or(from, |next| from + next.len_utf8())
    }

    /// Where a move back by word from `from` lands: past the spaces and line breaks before it, then to the start of the
    /// run of one class before that.
    #[must_use]
    pub fn word_back(&self, from: usize) -> usize {
        let mut before = self.text[..from].char_indices().rev().peekable();
        while before.next_if(|(_, character)| Class::of(*character).between()).is_some() {}
        let Some(&(_, first)) = before.peek() else {
            return 0;
        };
        let class = Class::of(first);
        let mut start = from;
        while let Some((at, _)) = before.next_if(|(_, character)| Class::of(*character) == class) {
            start = at;
        }

        start
    }

    /// Where a move forward by word from `from` lands: past the spaces and line breaks after it, then to the end of the
    /// run of one class after that.
    #[must_use]
    pub fn word_forward(&self, from: usize) -> usize {
        let mut after = self.text[from..].char_indices().map(|(at, character)| (from + at, character)).peekable();
        while after.next_if(|(_, character)| Class::of(*character).between()).is_some() {}
        let Some(&(_, first)) = after.peek() else {
            return self.text.len();
        };
        let class = Class::of(first);
        while after.next_if(|(_, character)| Class::of(*character) == class).is_some() {}

        after.peek().map_or(self.text.len(), |(at, _)| *at)
    }

    /// The run of one class a press at `at` lands in, as a double click selects it: the character after `at`'s, or the
    /// one before at a line's end, and the run of its class around it; a line break alone selects nothing.
    #[must_use]
    pub fn word_around(&self, at: usize) -> (usize, usize) {
        let next = self.text[at..].chars().next().filter(|character| *character != '\n');
        let pressed = next.map(|character| (at, character)).or_else(|| self.text[..at].char_indices().next_back());
        let Some((on, character)) = pressed else {
            return (at, at);
        };
        let class = Class::of(character);
        if class == Class::Break {
            return (at, at);
        }
        let same = |character: &char| Class::of(*character) == class;
        let start = self.text[..on]
            .char_indices()
            .rev()
            .take_while(|(_, character)| same(character))
            .last()
            .map_or(on, |(start, _)| start);
        let end = self.text[on..]
            .char_indices()
            .find(|(_, character)| !same(character))
            .map_or(self.text.len(), |(after, _)| on + after);

        (start, end)
    }

    /// The line between line breaks a press at `at` lands in, as a triple click selects it, without its break.
    #[must_use]
    pub fn line_around(&self, at: usize) -> (usize, usize) {
        let start = self.text[..at].rfind('\n').map_or(0, |found| found + 1);
        let end = self.text[at..].find('\n').map_or(self.text.len(), |found| at + found);

        (start, end)
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
