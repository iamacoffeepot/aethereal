//! The console (ADR-0053: The band and the console): the line being typed, with its history; the scrollback of what
//! was printed; and the engine's tools the line names, as their sheets last said. What it holds is plain state, worked
//! on by the overlay's actor as keys and answers come: what it prints and shows is decided here, and drawn there.
//!
//! ```text
//! Text ──> line ── Enter ──> command: help, clear, or a tool's CallTool ──> Called ──> printed into the scrollback
//! Tab ──> completion: a tool's name, a flag's name, or a variant's word
//! PageUp, PageDown, the wheel ──> the scrollback's view, held between its oldest row and its newest
//! the engine's recent log ──> a row each ──> the scrollback;  warnings and errors while closed ──> the band's count
//! a tool called ──> running, at the prompt's right end ──> its answer, refusal, or no answer lets it go
//! ```

mod command;
mod completion;
mod history;
mod line;
mod logs;
mod running;
mod scrollback;
#[cfg(test)]
mod tests;
mod wheel;

pub use command::Command;
pub use logs::{Alerts, Logged};
pub use scrollback::{Piece, Row};

use crate::colour::{BONE, BRICK, LICHEN, OCHRE, STONE};
use aethereal_data::{Budget, Plain, SchemaNodes};
use aethereal_tools::{Output, Sheets, print_image, print_value};
use completion::Completion;
use history::History;
use line::Line;
use running::Running;
use scrollback::Scrollback;
use wheel::Wheel;

/// What the line typed is shown after: the prompt, in lichen.
const PROMPT: &str = "> ";

/// The most bytes printing an answer may take: an answer is read whole into text.
const MOST_PRINT_BYTES: usize = 4 << 20;

/// A key the console handles, by what it means under the layout, as the W3C UI Events name it (ADR-0024).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    Up,
    Down,
    PageUp,
    PageDown,
    Tab,
    Enter,
}

impl Key {
    /// The key `key` names: none for one the console does not handle.
    pub fn of(key: &str) -> Option<Self> {
        let named = match key {
            "Backspace" => Self::Backspace,
            "Delete" => Self::Delete,
            "ArrowLeft" => Self::Left,
            "ArrowRight" => Self::Right,
            "Home" => Self::Home,
            "End" => Self::End,
            "ArrowUp" => Self::Up,
            "ArrowDown" => Self::Down,
            "PageUp" => Self::PageUp,
            "PageDown" => Self::PageDown,
            "Tab" => Self::Tab,
            "Enter" => Self::Enter,
            _ => return None,
        };

        Some(named)
    }
}

/// How much of the console shows: how many characters wide its rows are, and how many rows of the scrollback are in
/// view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct View {
    pub columns: usize,
    pub rows: usize,
}

/// Where the console's view of the scrollback stands: how many rows it keeps, and how many up from the newest the view
/// stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scrolled {
    pub kept: usize,
    pub up: usize,
}

/// What the next text typed is taken as.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
enum Typing {
    /// Typed into the line.
    Typed,
    /// Dropped, when it is the backtick's own: the backtick has just stepped the overlay, and the text it typed comes
    /// after it.
    AfterBacktick,
}

/// The console's state: the line, its history, the scrollback, the engine's tools, and the calls of them not yet
/// answered.
#[derive(Plain, Debug, Clone)]
pub struct Console {
    line: Line,
    history: History,
    scrollback: Scrollback,
    /// The engine's tools as the tools root last sent them: none, of revision zero, until it does.
    sheets: Sheets,
    typing: Typing,
    wheel: Wheel,
    running: Running,
}

impl Default for Console {
    fn default() -> Self {
        Self {
            line: Line::default(),
            history: History::default(),
            scrollback: Scrollback::default(),
            sheets: Sheets { revision: 0, tools: Vec::new() },
            typing: Typing::Typed,
            wheel: Wheel::default(),
            running: Running::default(),
        }
    }
}

impl Console {
    /// Takes in the engine's tools as the tools root now holds them.
    pub fn hear(&mut self, sheets: Sheets) {
        self.sheets = sheets;
    }

    /// The backtick has stepped the overlay: the text it typed, which comes next, is dropped.
    pub const fn stepped(&mut self) {
        self.typing = Typing::AfterBacktick;
    }

    /// Takes in `text`, typed: into the line at the cursor, unless it is the backtick's own.
    pub fn typed(&mut self, text: &str) {
        let backtick = self.typing == Typing::AfterBacktick && text == "`";
        self.typing = Typing::Typed;
        if backtick {
            return;
        }
        self.line.insert(text);
    }

    /// Takes in a key pressed, which any text typed after it belongs to: a key the console does not handle, `None`,
    /// does nothing else. Enter answers the line's command; every other key [`Command::Blank`].
    pub fn pressed(&mut self, key: Option<Key>, view: View) -> Command {
        self.typing = Typing::Typed;
        let Some(key) = key else {
            return Command::Blank;
        };
        match key {
            Key::Backspace => self.line.backspace(),
            Key::Delete => self.line.delete(),
            Key::Left => self.line.left(),
            Key::Right => self.line.right(),
            Key::Home => self.line.home(),
            Key::End => self.line.end(),
            Key::Up => {
                let typing = self.line.text();
                if let Some(older) = self.history.older(&typing) {
                    self.line.replace(older);
                }
            }
            Key::Down => {
                if let Some(newer) = self.history.newer() {
                    self.line.replace(&newer);
                }
            }
            Key::PageUp => self.scrollback.page_up(view.rows),
            Key::PageDown => self.scrollback.page_down(view.rows),
            Key::Tab => self.complete(view),
            Key::Enter => return self.enter(view),
        }

        Command::Blank
    }

    /// Takes in a turn of the wheel, `delta_pixels` down in the window's surface pixels, on a window whose notch is
    /// `notch_pixels`: the view scrolls three rows a notch, down toward the newest, and up toward the oldest when
    /// negative, held between them.
    pub fn wheeled(&mut self, delta_pixels: f32, notch_pixels: f32, view: View) {
        let rows = self.wheel.turned(delta_pixels, notch_pixels);
        let stopped = self.scrollback.scroll(rows, view.rows);
        if stopped {
            self.wheel.stopped();
        }
    }

    /// Where the view of the scrollback stands.
    pub fn scrolled(&self) -> Scrolled {
        Scrolled { kept: self.scrollback.len(), up: self.scrollback.up() }
    }

    /// Enters the line: it is kept in the history and echoed in lichen, the view goes to the newest row, and what its
    /// command comes to is printed, or answered for the overlay to call.
    fn enter(&mut self, view: View) -> Command {
        let entered = self.line.enter();
        self.history.keep(&entered);
        self.scrollback.scroll_to_newest();
        self.scrollback.print(&format!("{PROMPT}{entered}"), LICHEN, view.columns);

        let command = command::command(&entered, &self.sheets, view.columns);
        match &command {
            Command::Blank | Command::Call { .. } => {}
            Command::Shown(text) => self.scrollback.print(text, BONE, view.columns),
            Command::Clear => self.scrollback.clear(),
            Command::Refused(why) => self.scrollback.print(why, BRICK, view.columns),
        }

        command
    }

    /// Completes the word before the cursor: one match in place of it, several to what they all begin with, and
    /// listed on one line.
    fn complete(&mut self, view: View) {
        match completion::complete(&self.line.before(), &self.sheets.tools) {
            Completion::Nothing => {}
            Completion::One { from, word, whole } => {
                let spaced = if whole {
                    format!("{word} ")
                } else {
                    word
                };
                self.line.splice(from, &spaced);
            }
            Completion::Several { from, common, matches } => {
                self.line.splice(from, &common);
                self.scrollback.print(&matches.join("  "), STONE, view.columns);
            }
        }
    }

    /// Prints the value a tool answered, `bytes` of `output`'s schema, in the forms its flags read; an answer that
    /// prints as nothing prints nothing.
    pub fn answered(&mut self, output: &Output, bytes: &[u8], view: View) {
        let schema: &SchemaNodes = match output {
            Output::Value(schema) | Output::ValueOrImage(schema) => schema,
            Output::Image => {
                self.scrollback.print(&format!("an answer of {} bytes", bytes.len()), BONE, view.columns);
                return;
            }
        };
        match print_value(schema, bytes, &mut Budget::new(MOST_PRINT_BYTES)) {
            Ok(text) if text.is_empty() => {}
            Ok(text) => self.scrollback.print(&text, BONE, view.columns),
            Err(error) => self.refused(&format!("the answer did not read: {error}"), view),
        }
    }

    /// Prints the image a tool answered, as one line.
    pub fn answered_image(&mut self, png: &[u8], text: &str, view: View) {
        self.scrollback.print(&print_image(png, text), BONE, view.columns);
    }

    /// Prints why a tool, or the console, did not do what was asked, in brick.
    pub fn refused(&mut self, why: &str, view: View) {
        self.scrollback.print(why, BRICK, view.columns);
    }

    /// Prints a mark, `text` pinned `at_seconds` into the program, in ochre.
    pub fn marked(&mut self, text: &str, at_seconds: f64, view: View) {
        self.scrollback.print(&format!("mark at {at_seconds:.1} s: {text}"), OCHRE, view.columns);
    }

    /// Prints an entry of the engine's log.
    pub fn logged(&mut self, logged: &Logged, view: View) {
        self.scrollback.push(&logged.pieces(), view.columns);
    }

    /// Prints that `left` entries had left the engine's log before they were read, in stone.
    pub fn missed(&mut self, left: u64, view: View) {
        self.scrollback.print(&format!("{left} log entries left the log before they were read"), STONE, view.columns);
    }

    /// The rows in view, oldest first: the scrollback's newest, or those above as far as it is scrolled up.
    pub fn rows(&self, view: View) -> impl Iterator<Item = &Row> {
        self.scrollback.visible(view.rows)
    }

    /// The prompt's row: the prompt and the line in lichen, and, with the cursor at the line's end, the one word the
    /// word before it completes to, the rest of it in stone.
    pub fn prompt(&self) -> Vec<Piece> {
        let mut pieces = vec![Piece::new(PROMPT, LICHEN), Piece::new(&self.line.text(), LICHEN)];
        let at_end = self.line.at_end();
        if !at_end {
            return pieces;
        }
        let before = self.line.before();
        if let Completion::One { from, word, .. } = completion::complete(&before, &self.sheets.tools) {
            let typed = before.chars().count() - from;
            let rest: String = word.chars().skip(typed).collect();
            let ahead = typed > 0 && !rest.is_empty();
            if ahead {
                pieces.push(Piece::new(&rest, STONE));
            }
        }

        pieces
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

    /// The column the cursor stands at, counted from the prompt's first character.
    pub fn cursor_column(&self) -> usize {
        PROMPT.chars().count() + self.line.cursor()
    }
}
