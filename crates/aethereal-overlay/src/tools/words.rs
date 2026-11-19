//! The chat's words (ADR-0057: The console is the chat): `new_session`, `interrupt_session`, `hibernate_session`,
//! `wake_session`, and `archive_session`, the engine's own tools, which the overlay offers with its tools' worker once
//! the hub has shown it the chat, so only an engine the hub trusts offers them. Being tools, they run from any tab of
//! the console, `help` lists them, and Tab completes them, as every other command. The worker reads each, refuses one a session calls, and asks the overlay,
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
    /// Interrupt the running turn of the session named: the session carries on.
    Interrupt(Which),
    /// Hibernate the session named.
    Hibernate(Which),
    /// Wake the session named.
    Wake(Which),
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

/// The word that interrupts a session's turn.
pub const INTERRUPT_SESSION: &str = "interrupt_session";

/// The word that hibernates a session.
pub const HIBERNATE_SESSION: &str = "hibernate_session";

/// The word that wakes a session.
pub const WAKE_SESSION: &str = "wake_session";

/// The word that archives a session.
pub const ARCHIVE_SESSION: &str = "archive_session";

/// Makes a session named `name`, whose tab opens once the hub lists it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.new_session")]
pub struct NewSession {
    /// What the session is named, as its tab shows it. Quote it to say more than a word.
    #[flag(position)]
    pub name: String,
}

/// Interrupts the running turn of a session, as Escape on an empty line does at its tab: the session carries on.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.interrupt_session")]
pub struct InterruptSession {
    /// The session's name, as its tab shows it: the one whose tab is open when left out. An `Option` only because the
    /// command line reads a flag that may be left out as one; it is read once, as [`Which`], by [`Self::which`].
    #[flag(position)]
    session: Option<String>,
}

impl InterruptSession {
    /// The session it names.
    pub fn which(self) -> Which {
        Which::of(self.session)
    }
}

/// Hibernates a session, as one idle a while is: told, compacted, and its container stopped, its conversation kept.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.hibernate_session")]
pub struct HibernateSession {
    /// The session's name, as its tab shows it: the one whose tab is open when left out. An `Option` only because the
    /// command line reads a flag that may be left out as one; it is read once, as [`Which`], by [`Self::which`].
    #[flag(position)]
    session: Option<String>,
}

impl HibernateSession {
    /// The session it names.
    pub fn which(self) -> Which {
        Which::of(self.session)
    }
}

/// Wakes a hibernating session, its conversation resumed in a new container.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.wake_session")]
pub struct WakeSession {
    /// The session's name, as its tab shows it: the one whose tab is open when left out. An `Option` only because the
    /// command line reads a flag that may be left out as one; it is read once, as [`Which`], by [`Self::which`].
    #[flag(position)]
    session: Option<String>,
}

impl WakeSession {
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
    let interrupt = "Interrupts the running turn of the Claude session named, or of the one whose tab is open in this \
                     engine's console; the session carries on. Served only in an engine the hub trusts.";
    let hibernate = "Hibernates the Claude session named, or the one whose tab is open in this engine's console: it is \
                     told, compacted, and its container stopped, and a line or `wake_session` wakes it. Served only in \
                     an engine the hub trusts.";
    let wake = "Wakes the hibernating Claude session named, or the one whose tab is open in this engine's console, its \
                conversation resumed in a new container. Served only in an engine the hub trusts.";
    let archive = "Archives the Claude session named, or the one whose tab is open in this engine's console: its tab \
                   leaves every engine's console, and the hub keeps its transcript. Served only in an engine the hub \
                   trusts.";

    vec![
        sheet::<NewSession>(NEW_SESSION, new),
        sheet::<InterruptSession>(INTERRUPT_SESSION, interrupt),
        sheet::<HibernateSession>(HIBERNATE_SESSION, hibernate),
        sheet::<WakeSession>(WAKE_SESSION, wake),
        sheet::<ArchiveSession>(ARCHIVE_SESSION, archive),
    ]
}

/// The sheet of the word `name`, which reads a `K` and answers that it was done.
fn sheet<K: Flagged>(name: &str, description: &str) -> ToolSheet {
    ToolSheet::of::<K>(name, description, Output::Value(SchemaNodes::of(&Done::SHAPE)))
}
