//! The chat's words (ADR-0057: The console is the chat): `new_session`, `stop_session`, and `archive_session`, the
//! engine's own tools, which the overlay offers with its tools' worker once the hub has shown it the chat, so only an
//! engine the hub trusts offers them. Being tools, they run from any tab of the console, `help` lists them, and Tab
//! completes them, as every other command. The worker reads each, refuses one a session calls, and asks the overlay,
//! whose chat it changes, for the [`ChatWord`] it comes to; the overlay answers whether it told the link ([`Worded`]).

use crate::chat::Which;
use aethereal_data::config::Flagged;
use aethereal_data::{Plain, Schema, SchemaNodes};
use aethereal_tools::{Done, Output, ToolSheet};

/// One of the chat's words, as the owner called it, asked of the overlay, whose chat it changes: answered with
/// [`Worded`].
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.chat_word")]
pub enum ChatWord {
    /// Make a session named `name`.
    New { name: String },
    /// Stop the turn of the session named.
    Stop(Which),
    /// Archive the session named.
    Archive(Which),
}

/// What the overlay made of a [`ChatWord`]: the chat it came to told the link, or why it told nothing.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.worded")]
pub enum Worded {
    Told,
    Refused(String),
}

/// The word that makes a session.
pub const NEW_SESSION: &str = "new_session";

/// The word that stops a session's turn.
pub const STOP_SESSION: &str = "stop_session";

/// The word that archives a session.
pub const ARCHIVE_SESSION: &str = "archive_session";

/// The chat's words, each an engine's tool the overlay offers once the hub shows it the chat.
pub const CHAT_WORDS: [&str; 3] = [NEW_SESSION, STOP_SESSION, ARCHIVE_SESSION];

/// Makes a session named `name`, whose tab opens once the hub lists it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.new_session")]
pub struct NewSession {
    /// What the session is named, as its tab shows it. Quote it to say more than a word.
    #[flag(position)]
    pub name: String,
}

/// Stops the turn of a session, as Escape on an empty line does at its tab.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.stop_session")]
pub struct StopSession {
    /// The session's name, as its tab shows it: the one whose tab is open when left out. An `Option` only because the
    /// command line reads a flag that may be left out as one; it is read once, as [`Which`], by [`Self::which`].
    #[flag(position)]
    session: Option<String>,
}

impl StopSession {
    /// The session it names.
    pub fn which(self) -> Which {
        Which::of(self.session)
    }
}

/// Archives a session: its tab leaves every engine's console, and its transcript is kept.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.archive_session")]
pub struct ArchiveSession {
    /// The session's name, as its tab shows it: the one whose tab is open when left out. An `Option` only because the
    /// command line reads a flag that may be left out as one; it is read once, as [`Which`], by [`Self::which`].
    #[flag(position)]
    session: Option<String>,
}

impl ArchiveSession {
    /// The session it names.
    pub fn which(self) -> Which {
        Which::of(self.session)
    }
}

/// The sheets of the chat's words, offered apart from the worker's others, once the hub shows the overlay the chat.
pub fn chat_sheets() -> Vec<ToolSheet> {
    let new = "Makes a Claude session the hub runs, named as given, and opens its tab in this engine's console once the \
               hub lists it. Served only in an engine the hub trusts.";
    let stop = "Stops the turn of the Claude session named, or of the one whose tab is open in this engine's console. \
                Served only in an engine the hub trusts.";
    let archive = "Archives the Claude session named, or the one whose tab is open in this engine's console: its tab \
                   leaves every engine's console, and the hub keeps its transcript. Served only in an engine the hub \
                   trusts.";

    vec![
        sheet::<NewSession>(NEW_SESSION, new),
        sheet::<StopSession>(STOP_SESSION, stop),
        sheet::<ArchiveSession>(ARCHIVE_SESSION, archive),
    ]
}

/// The sheet of the word `name`, which reads a `K` and answers that it was done.
fn sheet<K: Flagged>(name: &str, description: &str) -> ToolSheet {
    ToolSheet::of::<K>(name, description, Output::Value(SchemaNodes::of(&Done::SHAPE)))
}
