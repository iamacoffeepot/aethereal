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
pub use scrollback::{Piece, Row, Shade};

use crate::colour::Ink;
use aethereal_data::{Budget, Plain, SchemaNodes};
use aethereal_tools::{Output, Sheets, print_image, print_value};
use completion::Completion;
use history::History;
use running::Running;
use scrollback::Scrollback;

/// The prompt mark after an owner's line's tag: who typed it, then this, then what they typed.
pub const PROMPT_MARK: &str = " >";

/// What an engine calls itself as it tags the owner's lines: those typed at it. In a transcript, every other engine is
/// called by its name.
pub const LOCAL: &str = "local";

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

    /// Empties the line, as Escape does with a session's tab open.
    pub fn clear_line(&mut self) {
        self.line.clear();
    }

    /// Keeps `typed` in the history, and empties the line: what entering a line does, whatever it comes to.
    pub fn kept(&mut self, typed: &str) {
        self.line.clear();
        self.history.keep(typed);
    }

    /// Enters `entered`, the line as its text input submitted it: it is kept in the history and echoed as the owner's
    /// line, the line is left empty, and what its command comes to is printed, or answered for the overlay to call. Rows are
    /// wrapped at `columns` characters.
    pub fn submitted(&mut self, entered: &str, columns: usize) -> Command {
        self.kept(entered);

        self.ran(entered, columns)
    }

    /// Runs `entered` as [`Self::submitted`] does, keeping nothing in the history: a `/` command typed with a session's
    /// tab open runs without its `/`, and its caller keeps it with it.
    pub fn ran(&mut self, entered: &str, columns: usize) -> Command {
        self.echo(entered, columns);

        let command = command::command(entered, &self.sheets, columns);
        match &command {
            Command::Blank | Command::Call { .. } => {}
            Command::Shown(text) => self.scrollback.print(text, Ink::Text, columns),
            Command::Clear => self.scrollback.clear(),
            Command::Refused(why) => self.scrollback.print(why, Ink::Error, columns),
        }

        command
    }

    /// Echoes `entered` as the owner's line, as a session's transcript shows one typed here: on the raised band, its
    /// tag, `local`, in the label's ink, and the prompt mark and its text in lichen, each of its lines wrapped a
    /// character short of `columns`, as the band's inset either side takes that room.
    fn echo(&mut self, entered: &str, columns: usize) {
        let width = columns.saturating_sub(1).max(1);
        let mut lines = entered.split('\n');
        let first = lines.next().unwrap_or_default();
        let tagged = [Piece::new(LOCAL, Ink::Label), Piece::new(&format!("{PROMPT_MARK} {first}"), Ink::Lichen)];
        self.scrollback.push(&tagged, Shade::Raised, width);
        for line in lines {
            self.scrollback.push(&[Piece::new(line, Ink::Lichen)], Shade::Raised, width);
        }
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
                self.scrollback.print(&matches.join("  "), Ink::Dim, columns);
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
    /// at `columns`; an answer that prints as nothing prints nothing. Answers what it printed.
    pub fn answered(&mut self, output: &Output, bytes: &[u8], columns: usize) -> String {
        let schema: &SchemaNodes = match output {
            Output::Value(schema) | Output::ValueOrImage(schema) => schema,
            Output::Image => {
                let printed = format!("an answer of {} bytes", bytes.len());
                self.scrollback.print(&printed, Ink::Text, columns);
                return printed;
            }
        };
        match print_value(schema, bytes, &mut Budget::new(MOST_PRINT_BYTES)) {
            Ok(text) => {
                if !text.is_empty() {
                    self.scrollback.print(&text, Ink::Text, columns);
                }
                text
            }
            Err(error) => {
                let why = format!("the answer did not read: {error}");
                self.refused(&why, columns);
                why
            }
        }
    }

    /// Prints the image a tool answered, as one line, and answers it.
    pub fn answered_image(&mut self, png: &[u8], text: &str, columns: usize) -> String {
        let printed = print_image(png, text);
        self.scrollback.print(&printed, Ink::Text, columns);

        printed
    }

    /// Prints why a tool, or the console, did not do what was asked, in the error's ink.
    pub fn refused(&mut self, why: &str, columns: usize) {
        self.scrollback.print(why, Ink::Error, columns);
    }

    /// Prints a mark, `text` pinned `at_seconds` into the program, in the mark's ink.
    pub fn marked(&mut self, text: &str, at_seconds: f64, columns: usize) {
        self.scrollback.print(&format!("mark at {at_seconds:.1} s: {text}"), Ink::Mark, columns);
    }

    /// Prints an entry of the engine's log.
    pub fn logged(&mut self, logged: &Logged, columns: usize) {
        self.scrollback.push(&logged.pieces(), Shade::Sheet, columns);
    }

    /// Prints that `left` entries had left the engine's log before they were read, dim.
    pub fn missed(&mut self, left: u64, columns: usize) {
        self.scrollback.print(&format!("{left} log entries left the log before they were read"), Ink::Dim, columns);
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

    /// What is running, dim, shown at the prompt's right end: nothing while no call is under way.
    pub fn running(&self) -> Vec<Piece> {
        vec![Piece::new(&self.running.shown(), Ink::Dim)]
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
