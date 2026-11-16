//! What a line typed with a session's tab open comes to (ADR-0057: The console is the chat): a line goes to Claude, a
//! line starting with `/` runs the engine's own tool at once, and a few `/` words are the chat's own or Claude Code's.
//!
//! ```text
//! "what do you see?"      ──> Say
//! "/capture_frame"        ──> Run "capture_frame", as the console runs a line, then Ran
//! "/compact", "/context"  ──> Say, the line as typed: Claude Code's own commands
//! "/stop"                 ──> Interrupt;   "/new <name>" ──> New;   "/archive" ──> Archive
//! ```

/// The `/` words that are Claude Code's own commands, sent to the session as a line rather than run here.
pub const CLAUDE_COMMANDS: &[&str] = &["compact", "context", "cost"];

/// The chat's word that stops the session's turn, as Escape on an empty line does.
const STOP: &str = "stop";

/// The chat's word that makes a session, named by the rest of the line.
const NEW: &str = "new";

/// The chat's word that archives the session whose tab is open.
const ARCHIVE: &str = "archive";

/// What a new session is named when `/new` names none.
const UNNAMED: &str = "session";

/// What a line typed with a session's tab open comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Typed {
    /// Nothing: a blank line.
    Blank,
    /// A line to the session, as typed.
    Say(String),
    /// A line of the console's, the `/` taken off, to run here as the console runs a line.
    Run(String),
    /// Stop the session's turn.
    Stop,
    /// A session made, so named.
    New(String),
    /// The open session archived.
    Archive,
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
    let (word, rest) = command.split_once(char::is_whitespace).unwrap_or((command, ""));
    match word {
        STOP => Typed::Stop,
        ARCHIVE => Typed::Archive,
        NEW => {
            let name = rest.trim();
            let named = if name.is_empty() {
                UNNAMED
            } else {
                name
            };
            Typed::New(String::from(named))
        }
        claude if CLAUDE_COMMANDS.contains(&claude) => Typed::Say(String::from(trimmed)),
        _ => Typed::Run(String::from(command)),
    }
}
