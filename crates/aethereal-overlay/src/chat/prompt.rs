//! What a line typed with a session's tab open comes to (ADR-0057: The console is the chat): a line goes to Claude, a
//! line starting with `/` runs the engine's own tool at once and is noted in the session, the chat's own words run
//! here alone, and a few `/` words are Claude Code's own.
//!
//! ```text
//! "what do you see?"            ──> Say
//! "/capture_frame"              ──> Run "capture_frame", as the console runs a line, then Ran
//! "/new work", "/stop"          ──> Own "new work": the chat's words, the engine's tools, run here and not noted
//! "/compact", "/context"        ──> Say, the line as typed: Claude Code's own commands
//! ```

use crate::tools::CHAT_WORDS;

/// The `/` words that are Claude Code's own commands, sent to the session as a line rather than run here.
pub const CLAUDE_COMMANDS: &[&str] = &["compact", "context", "cost"];

/// What a line typed with a session's tab open comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Typed {
    /// Nothing: a blank line.
    Blank,
    /// A line to the session, as typed.
    Say(String),
    /// A line of the console's, the `/` taken off, to run here as the console runs a line, and note in the session.
    Run(String),
    /// One of the chat's own words, the `/` taken off, to run here as the console runs a line: it is about the
    /// sessions, so it is not noted in one.
    Own(String),
}

/// What `line` comes to.
pub fn typed(line: &str) -> Typed {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Typed::Blank;
    }
    let Some(command) = trimmed.strip_prefix('/') else {
        return Typed::Say(String::from(line));
    };
    let word = command.split_whitespace().next().unwrap_or_default();
    match word {
        claude if CLAUDE_COMMANDS.contains(&claude) => Typed::Say(String::from(trimmed)),
        own if CHAT_WORDS.contains(&own) => Typed::Own(String::from(command)),
        _ => Typed::Run(String::from(command)),
    }
}
