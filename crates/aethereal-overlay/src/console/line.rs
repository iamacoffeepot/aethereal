//! The line being typed at the console (ADR-0053): its characters, and the cursor among them.

use aethereal_data::Plain;
use core::mem;

/// The line being typed: its characters, and where the cursor stands, as the number of characters before it.
#[derive(Plain, Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    characters: Vec<char>,
    cursor: u32,
}

impl Line {
    /// Types `text` at the cursor, which moves past it.
    pub fn insert(&mut self, text: &str) {
        let mut at = self.cursor();
        for character in text.chars() {
            self.characters.insert(at, character);
            at += 1;
        }
        self.set_cursor(at);
    }

    /// Deletes the character before the cursor: nothing at the line's start.
    pub fn backspace(&mut self) {
        let at_start = self.cursor == 0;
        if at_start {
            return;
        }
        self.cursor -= 1;
        self.characters.remove(self.cursor());
    }

    /// Deletes the character after the cursor: nothing at the line's end.
    pub fn delete(&mut self) {
        let at_end = self.cursor() >= self.characters.len();
        if at_end {
            return;
        }
        self.characters.remove(self.cursor());
    }

    /// Moves the cursor one character left, staying at the line's start.
    pub const fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// Moves the cursor one character right, staying at the line's end.
    pub fn right(&mut self) {
        self.set_cursor((self.cursor() + 1).min(self.characters.len()));
    }

    /// Moves the cursor to the line's start.
    pub const fn home(&mut self) {
        self.cursor = 0;
    }

    /// Moves the cursor to the line's end.
    pub fn end(&mut self) {
        self.set_cursor(self.characters.len());
    }

    /// The line as typed.
    pub fn text(&self) -> String {
        self.characters.iter().collect()
    }

    /// What is typed before the cursor.
    pub fn before(&self) -> String {
        self.characters[..self.cursor()].iter().collect()
    }

    /// How many characters are before the cursor.
    pub const fn cursor(&self) -> usize {
        self.cursor as usize
    }

    /// Puts the cursor `at` characters into the line: a line is typed by hand, far shorter than a u32 counts.
    fn set_cursor(&mut self, at: usize) {
        self.cursor = u32::try_from(at).unwrap_or(u32::MAX);
    }

    /// Whether the cursor is at the line's end.
    pub const fn at_end(&self) -> bool {
        self.cursor() == self.characters.len()
    }

    /// Puts `text` in place of the whole line, the cursor at its end: a command recalled from history.
    pub fn replace(&mut self, text: &str) {
        self.characters = text.chars().collect();
        self.set_cursor(self.characters.len());
    }

    /// Puts `word` in place of what is typed from the character `from` up to the cursor, which moves past it: a word
    /// completed.
    pub fn splice(&mut self, from: usize, word: &str) {
        let cursor = self.cursor();
        let from = from.min(cursor);
        self.characters.splice(from..cursor, word.chars());
        self.set_cursor(from + word.chars().count());
    }

    /// The line as typed, entered: the line is left empty for the next.
    pub fn enter(&mut self) -> String {
        self.cursor = 0;

        mem::take(&mut self.characters).into_iter().collect()
    }
}
