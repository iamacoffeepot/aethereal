//! A session's record (ADR-0057): what the hub keeps of a session beside its transcript, so a hub that starts again
//! knows its sessions and resumes each.

use crate::ContainerId;
use aethereal_data::Plain;

/// A session as the hub keeps it on disk.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.record")]
pub struct Record {
    /// What its caller named it: shown, and never part of its container.
    pub name: String,
    pub conversation: Conversation,
    /// How many containers it has run in, each a run.
    pub runs: u32,
    pub home: Home,
    pub kept: Standing,
}

/// Whether a session's home, its named volume, is the session's user's yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.home")]
pub enum Home {
    /// Its volume may not be made, or may still be root's, as Docker makes a new one: the next start makes it and gives
    /// it to the session's user first.
    Unowned,
    /// Its volume is made and given to the session's user, once, and every later container of it writes there.
    Owned,
}

/// The conversation Claude Code keeps for a session, in the session's home.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.conversation")]
pub enum Conversation {
    /// Claude has named none yet: the next container starts one.
    Unbegun,
    /// Claude named it by this id, a UUID: the next container resumes it.
    Begun(String),
}

/// Where a session stood when the hub last wrote its record.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.standing")]
pub enum Standing {
    /// Its container of the run `run` was made, and may run still.
    Running { container: ContainerId, run: u32 },
    /// No container of it runs: the next line resumes it.
    Stopped,
    /// It was archived: its container is gone, and it never runs again.
    Archived,
}
