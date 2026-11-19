//! The file capability's root (ADR-0032, ADR-0039, ADR-0054).

#[cfg(test)]
mod tests;

use crate::load::Load;
use crate::save::Save;
use crate::scan::Scan;
use crate::{FileRead, FileWritten, FilesListed, ListFiles, Namespace, ReadFile, WriteFile, Writes};
use aethereal_actor::{Answered, Bytes, Fault, Held, Later, Reply, Turn, actor};
use aethereal_data::{Bound, FilePath, Plain};
use std::collections::{BTreeMap, VecDeque};
use std::mem;

/// The file capability: reads, writes, and lists files for every other actor, in named namespaces, by path (ADR-0032,
/// ADR-0039, ADR-0054). An actor asks it a [`ReadFile`], a [`WriteFile`], or a [`ListFiles`] at its typed path,
/// `path::<Files>(())`, and is answered a [`FileRead`], a [`FileWritten`], or a [`FilesListed`] once the work is done.
///
/// It holds each request, runs a read, a write, or a listing for it, and answers it
/// with what that came to; one the host could not run is answered `Failed`. A write
/// to a namespace that is not written is answered [`FileWritten::ReadOnly`], and a
/// listing of one that is not listed [`FilesListed::NotListed`], in the turn that
/// asks, with no job run.
///
/// It runs one write of a file at a time: a write asked of a file while another of it runs waits until that one is
/// answered, so a file's writes land in the order they were asked, whichever of its threads a host would finish first
/// (ADR-0039).
#[derive(Bound)]
#[kind(name = "aether.files")]
pub struct Files {
    /// The reads it holds, each by the number of the job it ran for it.
    waiting: BTreeMap<u64, Held<FileRead>>,
    /// The writes it holds: for each file it runs a write of, that write and those asked of the file after it. A list
    /// found by file, as a path is no map key.
    writing: Vec<Saves>,
    /// The listings it holds, each by the number of the job it ran for it.
    listing: BTreeMap<u64, Held<FilesListed>>,
    /// The number of the next job it runs.
    next: u64,
}

/// What the file capability is made from: nothing, its host's storage being the host's.
#[derive(Debug, Default, PartialEq, Eq, Plain)]
#[kind(name = "aether.files.config")]
pub struct FilesConfig;

/// What the file capability keeps for a read it runs: which request it was run for.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.files.reading")]
pub struct Reading {
    number: u64,
}

/// What the file capability keeps for a write it runs: the file it writes, which runs no other write until it is
/// answered.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.files.saving")]
pub struct Saving {
    namespace: Namespace,
    path: FilePath,
}

impl Saving {
    /// The job that writes `bytes` to this file, as `how` says.
    fn save(&self, bytes: Bytes, how: Writes) -> Save {
        Save::new(self.namespace, self.path.clone(), bytes, how)
    }
}

/// The writes of one file the file capability holds: the one running, and those asked after it, in the order asked.
#[derive(Bound)]
struct Saves {
    /// The file they write.
    file: Saving,
    /// Whoever asked the write running.
    running: Held<FileWritten>,
    /// The writes asked after it, each run once the one before it is answered.
    queued: VecDeque<Queued>,
}

/// A write asked of a file while another of it runs: whoever asked it, and what it writes, as it asks.
#[derive(Bound)]
struct Queued {
    held: Held<FileWritten>,
    bytes: Bytes,
    how: Writes,
}

impl Saves {
    /// The writes of `file`, whose one write runs for `running`.
    const fn new(file: Saving, running: Held<FileWritten>) -> Self {
        Self { file, running, queued: VecDeque::new() }
    }

    /// Whether they are the writes of `file`.
    fn writes(&self, file: &Saving) -> bool {
        self.file == *file
    }

    /// Queues `queued` behind the writes asked before it.
    fn queue(&mut self, queued: Queued) {
        self.queued.push_back(queued);
    }

    /// The write queued first, taken from the queue: none when no write waits.
    fn next(&mut self) -> Option<Queued> {
        self.queued.pop_front()
    }

    /// Makes the write for `held` the one running, and answers with whoever asked the one that ran.
    fn runs(&mut self, held: Held<FileWritten>) -> Held<FileWritten> {
        mem::replace(&mut self.running, held)
    }

    /// Whoever asked the write running, once no other write of the file waits.
    fn last(self) -> Held<FileWritten> {
        self.running
    }
}

/// What the file capability keeps for a listing it runs: which request it was run for.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.files.scanning")]
pub struct Scanning {
    number: u64,
}

#[actor]
impl Files {
    fn begin(_: FilesConfig) -> Result<Self, Fault> {
        Ok(Self { waiting: BTreeMap::new(), writing: Vec::new(), listing: BTreeMap::new(), next: 0 })
    }

    #[request]
    fn read(&mut self, read: ReadFile, turn: &mut Turn<'_, Self>) -> Result<Later<FileRead>, Fault> {
        let (held, later) = turn.hold();
        let number = self.number();
        turn.run(&Load::new(read.namespace, read.path), &Reading { number })?;
        self.waiting.insert(number, held);

        Ok(later)
    }

    #[answer]
    fn loaded(
        &mut self,
        (answered, reading): (Answered<FileRead>, Reading),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(held) = self.waiting.remove(&reading.number) else {
            return Ok(());
        };
        let read = match answered {
            Answered::Yes(read) => read,
            Answered::No(_) => FileRead::Failed,
        };
        turn.answer(held, &read)?;

        Ok(())
    }

    #[request]
    fn write(&mut self, write: WriteFile, turn: &mut Turn<'_, Self>) -> Result<Reply<FileWritten>, Fault> {
        if !write.namespace.is_written() {
            return Ok(Reply::Now(FileWritten::ReadOnly));
        }
        let (held, later) = turn.hold();
        let WriteFile { namespace, path, bytes, how } = write;
        let file = Saving { namespace, path };
        let running = self.writing.iter_mut().find(|saves| saves.writes(&file));
        if let Some(saves) = running {
            saves.queue(Queued { held, bytes, how });
            return Ok(Reply::Later(later));
        }
        turn.run(&file.save(bytes, how), &file)?;
        self.writing.push(Saves::new(file, held));

        Ok(Reply::Later(later))
    }

    #[answer]
    fn saved(
        &mut self,
        (answered, saving): (Answered<FileWritten>, Saving),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(at) = self.writing.iter().position(|saves| saves.writes(&saving)) else {
            return Ok(());
        };
        let written = match answered {
            Answered::Yes(written) => written,
            Answered::No(_) => FileWritten::Failed,
        };
        // The file's next write runs only now the one before it is answered, so the two never land out of order.
        let Some(Queued { held, bytes, how }) = self.writing[at].next() else {
            turn.answer(self.writing.swap_remove(at).last(), &written)?;
            return Ok(());
        };
        turn.run(&saving.save(bytes, how), &saving)?;
        turn.answer(self.writing[at].runs(held), &written)?;

        Ok(())
    }

    #[request]
    fn list(&mut self, list: ListFiles, turn: &mut Turn<'_, Self>) -> Result<Reply<FilesListed>, Fault> {
        if !list.namespace.is_listed() {
            return Ok(Reply::Now(FilesListed::NotListed));
        }
        let (held, later) = turn.hold();
        let number = self.number();
        turn.run(&Scan::new(list.namespace), &Scanning { number })?;
        self.listing.insert(number, held);

        Ok(Reply::Later(later))
    }

    #[answer]
    fn scanned(
        &mut self,
        (answered, scanning): (Answered<FilesListed>, Scanning),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(held) = self.listing.remove(&scanning.number) else {
            return Ok(());
        };
        // A listing larger than the engine's `read_limit` reaches here unreadable, and is answered as one that failed.
        let listed = match answered {
            Answered::Yes(listed) => listed,
            Answered::No(_) => FilesListed::Failed,
        };
        turn.answer(held, &listed)?;

        Ok(())
    }

    /// The number of the next job it runs, taken.
    const fn number(&mut self) -> u64 {
        let number = self.next;
        self.next += 1;

        number
    }
}
