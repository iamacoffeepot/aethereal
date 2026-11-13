//! The console (ADR-0053: The band and the console): the line being typed, with its history; the scrollback of what
//! was printed; and the engine's tools the line names, as their sheets last said. What it holds is plain state, worked
//! on by the overlay's actor as its line's text input raises its edits and its submit, as its keys come, and as answers
//! come: what it prints and shows is decided here, and laid out and drawn by the overlay's screen. The line's caret
//! and its keys are its text input's (ADR-0056: Stock widgets), and the scrollback's view, its bar, and the wheel are
//! the scroll it is shown in.
//!
//! ```text
//! the line's text input ── an edit ──> line;  Enter ──> command: help, clear, or a tool's CallTool ──> Called ──> printed
//! Tab ──> completion of the word before the caret: a tool's name, a flag's name, or a variant's word
//! Up, Down ──> the history
//! the engine's recent log ──> a row each ──> the scrollback;  warnings and errors while closed ──> the band's count
//! a tool called ──> running, at the prompt's right end ──> its answer, refusal, or no answer lets it go
//! ```

mod command;
mod completion;
mod history;
mod logs;
mod running;
mod scrollback;
#[cfg(test)]
mod tests;

pub use command::Command;
pub use logs::{Alerts, Logged};
pub use scrollback::{Piece, Row};

use crate::colour::{BONE, BRICK, LICHEN, OCHRE, STONE};
use aethereal_data::{Budget, Plain, SchemaNodes};
use aethereal_tools::{Output, Sheets, print_image, print_value};
use completion::Completion;
use history::History;
use running::Running;
use scrollback::Scrollback;

/// What the line typed is shown after: the prompt, in lichen.
const PROMPT: &str = "> ";

/// The most bytes printing an answer may take: an answer is read whole into text.
const MOST_PRINT_BYTES: usize = 4 << 20;

/// The console's state: the line, its history, the scrollback, the engine's tools, and the calls of them not yet
/// answered.
#[derive(Plain, Debug, Clone)]
pub struct Console {
    line: String,
    history: History,
    scrollback: Scrollback,
    /// The engine's tools as the tools root last sent them: none, of revision zero, until it does.
    sheets: Sheets,
    running: Running,
}

impl Default for Console {
    fn default() -> Self {
        Self {
            line: String::new(),
            history: History::default(),
            scrollback: Scrollback::default(),
            sheets: Sheets { revision: 0, tools: Vec::new() },
            running: Running::default(),
        }
    }
}

impl Console {
    /// Takes in the engine's tools as the tools root now holds them.
    pub fn hear(&mut self, sheets: Sheets) {
        self.sheets = sheets;
    }

    /// The line as typed.
    pub fn line(&self) -> &str {
        &self.line
    }

    /// Takes in the line as its text input edited it.
    pub fn edited(&mut self, line: String) {
        self.line = line;
    }

    /// Enters `entered`, the line as its text input submitted it: it is kept in the history and echoed in lichen, the
    /// line is left empty, and what its command comes to is printed, or answered for the overlay to call. Rows are
    /// wrapped at `columns` characters.
    pub fn submitted(&mut self, entered: &str, columns: usize) -> Command {
        self.line.clear();
        self.history.keep(entered);
        self.scrollback.print(&format!("{PROMPT}{entered}"), LICHEN, columns);

        let command = command::command(entered, &self.sheets, columns);
        match &command {
            Command::Blank | Command::Call { .. } => {}
            Command::Shown(text) => self.scrollback.print(text, BONE, columns),
            Command::Clear => self.scrollback.clear(),
            Command::Refused(why) => self.scrollback.print(why, BRICK, columns),
        }

        command
    }

    /// Puts the command one older than the one recalled in place of the line, keeping the line as it stood to come back
    /// to: nothing when there is no older.
    pub fn older(&mut self) {
        if let Some(older) = self.history.older(&self.line) {
            self.line = String::from(older);
        }
    }

    /// Puts the command one newer than the one recalled in place of the line, or the line that was being typed past the
    /// newest: nothing when nothing is recalled.
    pub fn newer(&mut self) {
        if let Some(newer) = self.history.newer() {
            self.line = newer;
        }
    }

    /// Completes the word before `caret`, a byte offset into the line: one match in place of it, several to what they
    /// all begin with, and listed on one line wrapped at `columns`.
    pub fn complete(&mut self, caret: usize, columns: usize) {
        let caret = boundary(&self.line, caret);
        let (before, after) = self.line.split_at(caret);
        let (from, word) = match completion::complete(before, &self.sheets.tools) {
            Completion::Nothing => return,
            Completion::One { from, word, whole } => {
                let spaced = if whole {
                    format!("{word} ")
                } else {
                    word
                };
                (from, spaced)
            }
            Completion::Several { from, common, matches } => {
                self.scrollback.print(&matches.join("  "), STONE, columns);
                (from, common)
            }
        };
        let kept: String = before.chars().take(from).collect();
        self.line = format!("{kept}{word}{after}");
    }

    /// What the one word the word before `caret` completes to adds to it, with the caret at the line's end: nothing
    /// with the caret elsewhere, a word that does not complete, or one that completes several ways.
    pub fn completion_ahead(&self, caret: usize) -> String {
        let at_end = caret >= self.line.len();
        if !at_end {
            return String::new();
        }
        let Completion::One { from, word, .. } = completion::complete(&self.line, &self.sheets.tools) else {
            return String::new();
        };
        let typed = self.line.chars().count() - from;
        let rest: String = word.chars().skip(typed).collect();
        let ahead = typed > 0 && !rest.is_empty();
        if !ahead {
            return String::new();
        }

        rest
    }

    /// Prints the value a tool answered, `bytes` of `output`'s schema, in the forms its flags read, its rows wrapped
    /// at `columns`; an answer that prints as nothing prints nothing.
    pub fn answered(&mut self, output: &Output, bytes: &[u8], columns: usize) {
        let schema: &SchemaNodes = match output {
            Output::Value(schema) | Output::ValueOrImage(schema) => schema,
            Output::Image => {
                self.scrollback.print(&format!("an answer of {} bytes", bytes.len()), BONE, columns);
                return;
            }
        };
        match print_value(schema, bytes, &mut Budget::new(MOST_PRINT_BYTES)) {
            Ok(text) if text.is_empty() => {}
            Ok(text) => self.scrollback.print(&text, BONE, columns),
            Err(error) => self.refused(&format!("the answer did not read: {error}"), columns),
        }
    }

    /// Prints the image a tool answered, as one line.
    pub fn answered_image(&mut self, png: &[u8], text: &str, columns: usize) {
        self.scrollback.print(&print_image(png, text), BONE, columns);
    }

    /// Prints why a tool, or the console, did not do what was asked, in brick.
    pub fn refused(&mut self, why: &str, columns: usize) {
        self.scrollback.print(why, BRICK, columns);
    }

    /// Prints a mark, `text` pinned `at_seconds` into the program, in ochre.
    pub fn marked(&mut self, text: &str, at_seconds: f64, columns: usize) {
        self.scrollback.print(&format!("mark at {at_seconds:.1} s: {text}"), OCHRE, columns);
    }

    /// Prints an entry of the engine's log.
    pub fn logged(&mut self, logged: &Logged, columns: usize) {
        self.scrollback.push(&logged.pieces(), columns);
    }

    /// Prints that `left` entries had left the engine's log before they were read, in stone.
    pub fn missed(&mut self, left: u64, columns: usize) {
        self.scrollback.print(&format!("{left} log entries left the log before they were read"), STONE, columns);
    }

    /// Every row kept, oldest first.
    pub fn rows(&self) -> impl ExactSizeIterator<Item = &Row> {
        self.scrollback.rows()
    }

    /// Counts a call of `tool`, asked of the engine's tools root, as running until it is answered.
    pub fn called(&mut self, tool: &str) {
        self.running.called(tool);
    }

    /// Lets go of a call of `tool`: it was answered, refused, or will not be answered.
    pub fn settled(&mut self, tool: &str) {
        self.running.settled(tool);
    }

    /// What is running, in stone, shown at the prompt's right end: nothing while no call is under way.
    pub fn running(&self) -> Vec<Piece> {
        vec![Piece::new(&self.running.shown(), STONE)]
    }
}

/// `offset`, pulled back to the nearest character's boundary of `text` at or before it, and no further than its end.
fn boundary(text: &str, offset: usize) -> usize {
    let mut at = offset.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }

    at
}
