//! What a line typed with a session's tab open comes to (ADR-0057: The console is the chat), by the tab's mode: in chat
//! mode a line is said to Claude, a `/` line among them, which Claude Code reads as its own command; in `!` mode a line
//! runs the engine's own tool here, and it and what it printed are sent to the session.
//!
//! ```text
//! chat  "what do you see?"            ──> Say, as typed
//! chat  "/compact", "/clear"          ──> Say, as typed: Claude Code's
//! !     "capture_frame", "clear"      ──> Run, as the console runs a line, then Ran
//! !     "new_session work"            ──> Run: the chat's words too, one rule
//! any   "   "                         ──> Blank
//! ```

use crate::console::Mode;

/// What a line typed with a session's tab open comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Typed {
    /// Nothing: a blank line.
    Blank,
    /// A line to the session, as typed.
    Say(String),
    /// A line of the console's, trimmed, to run here as the console runs a line, and send to the session with what it
    /// printed.
    Run(String),
}

/// What `line`, typed in `mode`, comes to.
pub fn typed(line: &str, mode: Mode) -> Typed {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Typed::Blank;
    }

    match mode {
        Mode::Chat => Typed::Say(String::from(line)),
        Mode::Bang => Typed::Run(String::from(trimmed)),
    }
}
