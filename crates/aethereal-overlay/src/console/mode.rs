//! The prompt's mode (ADR-0057: The console is the chat): what a line entered comes to, shown as the prompt's mark. The
//! console's own tab is always in `!` mode; a session's tab is in chat mode until `!` typed on an empty line puts it in
//! `!` mode, which Backspace or Escape on an empty line leaves.

use aethereal_data::Plain;

/// What a line entered at a tab comes to.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Said to the session whose tab is open, a `/` line passed whole to Claude Code: marked `>`.
    Chat,
    /// Run here as one of the engine's tools: marked `!`.
    Bang,
}

impl Mode {
    /// The prompt's mark in this mode.
    pub const fn mark(self) -> &'static str {
        match self {
            Self::Chat => ">",
            Self::Bang => "!",
        }
    }

    /// The mark after an owner's line's tag, who typed it, then this, then what they typed: the mark a word's space
    /// after the tag.
    pub const fn tag_mark(self) -> &'static str {
        match self {
            Self::Chat => " >",
            Self::Bang => " !",
        }
    }
}
