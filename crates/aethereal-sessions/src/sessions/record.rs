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
    pub kept: Standing,
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
