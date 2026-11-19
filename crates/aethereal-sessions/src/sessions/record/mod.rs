//! A session's record (ADR-0057): what the hub keeps of a session beside its transcript, so a hub that starts again
//! knows its sessions and picks each up. A session's container exists only while its record says it may: the record
//! saying a run starts lands before anything of that run is made, and the record saying the session hibernates is
//! written only once its container is removed.

#[cfg(test)]
mod tests;

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

/// Where a session stood when the hub last wrote its record: awake, with a container of it that may exist and what a
/// hub that starts again finds it by among the containers Docker lists; hibernating; or archived. Each case keeps its
/// place, as a record is read by the place of its case, so a record written before a case was renamed still reads.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.standing")]
pub enum Standing {
    /// Awake: its container of the run `run` was made, and may run still.
    Awake { container: ContainerId, run: u32 },
    /// No container of it exists: it hibernates, and the next line wakes it.
    Hibernating,
    /// It was archived: its container is gone, and it never wakes again.
    Archived,
    /// Awake: its container of the run `run` may be being made, under the name its number and the run give it, as
    /// Docker has not yet answered its id. Last of the cases, so a record written before there was one still reads.
    Starting { run: u32 },
}
