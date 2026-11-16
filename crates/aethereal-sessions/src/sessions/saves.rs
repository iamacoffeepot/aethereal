//! Where the capability keeps its sessions (ADR-0057): `Files`' `Save` namespace, asked as any other actor asks it.
//! `sessions/index` lists every session's number and is written whole as a session is added; a session's record is
//! `sessions/<n>/record`, written whole; and its transcript `sessions/<n>/transcript`, each entry a frame appended to it.
//! One write runs at a time, so a transcript's append is answered before its record is written, and each lands in the
//! order made.

use super::{Does, Sessions, Unfound};
use aethereal_actor::{Answer, Answered, Fault, Handles, Level, Turn, path};
use aethereal_data::{DecodeError, FilePath, Plain, to_bytes};
use aethereal_files::{FileWritten, Files, Namespace, WriteFile, Writes};
use core::mem;

/// The file that lists every session's number.
pub(super) const INDEX: FilePath = FilePath::new("sessions/index");

/// Every session the hub keeps, by its number: what the index file holds.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.index")]
pub struct Index {
    pub sessions: Vec<u32>,
}

/// What the capability keeps for a read of its files as it begins: which file it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.loading")]
pub enum Loading {
    Index,
    Record { session: u32 },
    Transcript { session: u32 },
}

/// What the capability keeps for a write of one of its files: nothing, as one runs at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.keeping")]
pub struct Keeping;

/// The record of the session numbered `session`.
pub(super) fn record(session: u32) -> Result<FilePath, DecodeError> {
    FilePath::parse(format!("sessions/{session}/record"))
}

/// The transcript of the session numbered `session`.
pub(super) fn transcript(session: u32) -> Result<FilePath, DecodeError> {
    FilePath::parse(format!("sessions/{session}/transcript"))
}

/// One write of the capability's files, waiting its turn.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub(super) struct Put {
    path: FilePath,
    how: Writes,
    bytes: Vec<u8>,
}

/// The capability's writes to `Files`, one at a time.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub(super) enum Lane {
    /// None runs.
    Idle,
    /// The write of `file` runs, and `next` wait their turn, in order.
    Writing { file: FilePath, next: Vec<Put> },
}

/// Whether the index file lists every session the capability holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub(super) enum Indexed {
    /// It does, or its write is made.
    Written,
    /// A session was added since its last write was made.
    Changed,
}

impl Handles<Answer<FileWritten, Keeping>> for Sessions {
    type Sends = super::Acts;

    /// A write is done: the next is sent, and once none waits, those of what changed meanwhile.
    fn handle(
        &mut self,
        (answered, Keeping): (Answered<FileWritten>, Keeping),
        turn: &mut Turn<'_, Self, super::Acts>,
    ) -> Result<(), Fault> {
        let Lane::Writing { file, next } = mem::replace(&mut self.lane, Lane::Idle) else {
            return Ok(());
        };
        let is_written = answered == Answered::Yes(FileWritten::Written);
        if !is_written {
            turn.log(Level::Error, format_args!("the sessions' file `{file}` was not written: {answered:?}"));
        }
        if next.is_empty() {
            return self.flush(turn);
        }

        self.send(turn, next)
    }
}

impl Sessions {
    /// Sends the writes of what has changed, unless one runs: the index first when a session was added, then each
    /// changed session's new transcript frames and its record.
    pub(super) fn flush<S: Does>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault> {
        let is_writing = matches!(self.lane, Lane::Writing { .. });
        if is_writing {
            return Ok(());
        }
        let puts = self.puts()?;

        self.send(turn, puts)
    }

    /// The writes of what has changed, in the order they are to land, each change taken as written.
    fn puts(&mut self) -> Result<Vec<Put>, Fault> {
        let mut puts = Vec::new();
        if self.indexed == Indexed::Changed {
            let index = Index { sessions: self.held.keys().copied().collect() };
            puts.push(Put { path: INDEX, how: Writes::Whole, bytes: to_bytes(&index)? });
            self.indexed = Indexed::Written;
        }
        for (session, frames) in mem::take(&mut self.unwritten) {
            let Some(held) = self.held.get(&session) else {
                continue;
            };
            if !frames.is_empty() {
                puts.push(Put { path: transcript(session)?, how: Writes::Append, bytes: frames });
            }
            puts.push(Put { path: record(session)?, how: Writes::Whole, bytes: to_bytes(&held.record())? });
        }

        Ok(puts)
    }

    /// Sends the first of `puts`, the rest to follow it one at a time, each once the one before is answered.
    fn send<S: Does>(&mut self, turn: &mut Turn<'_, Self, S>, puts: Vec<Put>) -> Result<(), Fault> {
        let mut puts = puts.into_iter();
        let Some(Put { path: file, how, bytes }) = puts.next() else {
            return Ok(());
        };
        let Ok(bytes) = turn.put(bytes) else {
            return Err(Fault::new(&Unfound::NoRoom)?);
        };
        let write = WriteFile { namespace: Namespace::Save, path: file.clone(), bytes, how };
        turn.to(path::<Files>(())).ask(&write, &Keeping)?;
        self.lane = Lane::Writing { file, next: puts.collect() };

        Ok(())
    }
}
