//! Where the capability keeps its sessions (ADR-0057): `Files`' `Save` namespace, asked as any other actor asks it.
//! `sessions/index` lists every session's number and is written whole as a session is added; a session's record is
//! `sessions/<n>/record`, written whole; and its transcript `sessions/<n>/transcript`, each entry a frame appended to it.
//! One write runs at a time, so a transcript's append is answered before its record is written, and each lands in the
//! order made. A record that says a run starts claims no entry, and the start of that run waits for it to land, so a
//! container is never made that the record on disk does not name.

use super::asks::Refusal;
use super::session::Step;
use super::{Sessions, Unfound};
use aethereal_actor::{Answered, Fault, Level, Turn, path};
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

/// One write of the capability's files, waiting its turn, and what it lets go ahead once it lands.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub(super) struct Put {
    path: FilePath,
    how: Writes,
    bytes: Vec<u8>,
    landing: Landing,
}

/// What waits on a write landing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub(super) enum Landing {
    /// Nothing.
    Nothing,
    /// The start of `session`'s run `run`, which this write of its record says it starts.
    Starting { session: u32, run: u32 },
}

/// The capability's writes to `Files`, one at a time.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub(super) enum Lane {
    /// None runs.
    Idle,
    /// The write of `file` runs, `landing` waiting on it, and `next` wait their turn, in order.
    Writing { file: FilePath, landing: Landing, next: Vec<Put> },
}

/// Whether the index file lists every session the capability holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub(super) enum Indexed {
    /// It does, or its write is made.
    Written,
    /// A session was added since its last write was made.
    Changed,
}

impl Sessions {
    /// A write is done: what waited on it goes ahead, or is refused when it was not written; then the next is sent,
    /// and once none waits, those of what changed meanwhile.
    pub(super) fn handle_file_written(
        &mut self,
        (answered, Keeping): (Answered<FileWritten>, Keeping),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Lane::Writing { file, landing, next } = mem::replace(&mut self.lane, Lane::Idle) else {
            return Ok(());
        };
        let is_written = answered == Answered::Yes(FileWritten::Written);
        if !is_written {
            turn.log(Level::Error, format_args!("the sessions' file `{file}` was not written: {answered:?}"));
        }
        if let Landing::Starting { session, run } = landing {
            self.recorded(turn, session, run, &answered)?;
        }
        if next.is_empty() {
            return self.flush(turn);
        }

        self.send(turn, next)
    }

    /// The write of `session`'s record, saying it starts its run `run`, is `answered`: once it has landed, the run's
    /// volumes are made, unless the hub's repository is not served, when the start is refused before anything of it
    /// is made, as its container's bootstrap would fail; a record that did not land says nothing of the run, so
    /// nothing of it is made.
    fn recorded(
        &mut self,
        turn: &mut Turn<'_, Self>,
        session: u32,
        run: u32,
        answered: &Answered<FileWritten>,
    ) -> Result<(), Fault> {
        let is_recording = self.is_at(session, run, |step| matches!(step, Step::Recording));
        if !is_recording {
            return Ok(());
        }

        match (answered, self.may_serve()) {
            (Answered::Yes(FileWritten::Written), Ok(())) => self.house(turn, session, run, 0),
            (Answered::Yes(FileWritten::Written), Err(why)) => self.unstarted(turn, session, run, &why),
            (Answered::Yes(_) | Answered::No(_), _) => self.unstarted(turn, session, run, &Refusal::Unrecorded),
        }
    }
}

impl Sessions {
    /// Sends the writes of what has changed, unless one runs: the index first when a session was added, then each
    /// changed session's new transcript frames and its record.
    pub(super) fn flush(&mut self, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.show(turn)?;
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
            puts.push(Put { path: INDEX, how: Writes::Whole, bytes: to_bytes(&index)?, landing: Landing::Nothing });
            self.indexed = Indexed::Written;
        }
        for (session, frames) in mem::take(&mut self.unwritten) {
            let Some(held) = self.held.get(&session) else {
                continue;
            };
            if !frames.is_empty() {
                let path = transcript(session)?;
                puts.push(Put { path, how: Writes::Append, bytes: frames, landing: Landing::Nothing });
            }
            let bytes = to_bytes(&held.record())?;
            puts.push(Put { path: record(session)?, how: Writes::Whole, bytes, landing: held.landing(session) });
        }

        Ok(puts)
    }

    /// Sends the first of `puts`, the rest to follow it one at a time, each once the one before is answered.
    fn send(&mut self, turn: &mut Turn<'_, Self>, puts: Vec<Put>) -> Result<(), Fault> {
        let mut puts = puts.into_iter();
        let Some(Put { path: file, how, bytes, landing }) = puts.next() else {
            return Ok(());
        };
        let Ok(bytes) = turn.put(bytes) else {
            return Err(Fault::new(&Unfound::NoRoom)?);
        };
        let write = WriteFile { namespace: Namespace::Save, path: file.clone(), bytes, how };
        turn.to(path::<Files>(())).ask(&write, &Keeping)?;
        self.lane = Lane::Writing { file, landing, next: puts.collect() };

        Ok(())
    }
}
