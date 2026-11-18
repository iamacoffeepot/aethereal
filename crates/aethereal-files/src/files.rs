//! The file capability's root (ADR-0032, ADR-0039, ADR-0054).

use crate::load::Load;
use crate::save::Save;
use crate::scan::Scan;
use crate::{FileRead, FileWritten, FilesListed, ListFiles, ReadFile, WriteFile};
use aethereal_actor::{
    Actor, Answer, Answered, Anyone, Beginning, ChildOf, Ending, Fault, Handles, Held, Later, Receives, Reply, Top,
    Turn,
};
use aethereal_data::{Bound, Plain};
use std::collections::BTreeMap;

/// The file capability: reads, writes, and lists files for every other actor, in named namespaces, by path (ADR-0032,
/// ADR-0039, ADR-0054). An actor asks it a [`ReadFile`], a [`WriteFile`], or a [`ListFiles`] at its typed path,
/// `path::<Files>(())`, and is answered a [`FileRead`], a [`FileWritten`], or a [`FilesListed`] once the work is done.
///
/// It holds each request, runs a read, a write, or a listing for it, and answers it
/// with what that came to; one the host could not run is answered `Failed`. A write
/// to a namespace that is not written is answered [`FileWritten::ReadOnly`], and a
/// listing of one that is not listed [`FilesListed::NotListed`], in the turn that
/// asks, with no job run.
#[derive(Bound)]
#[kind(name = "aether.files")]
pub struct Files {
    /// The reads it holds, each by the number of the job it ran for it.
    waiting: BTreeMap<u64, Held<FileRead>>,
    /// The writes it holds, each by the number of the job it ran for it.
    writing: BTreeMap<u64, Held<FileWritten>>,
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

/// What the file capability keeps for a write it runs: which request it was run for.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.files.saving")]
pub struct Saving {
    number: u64,
}

/// What the file capability keeps for a listing it runs: which request it was run for.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.files.scanning")]
pub struct Scanning {
    number: u64,
}

impl ChildOf for Files {
    type Parent = Top;
}

impl Actor for Files {
    type Config = FilesConfig;
    type Key = ();
    type Handles = (
        ReadFile,
        Answer<FileRead, Reading>,
        WriteFile,
        Answer<FileWritten, Saving>,
        ListFiles,
        Answer<FilesListed, Scanning>,
    );
    type Depends = ();
    type Spawns = ();

    fn begin(_: FilesConfig, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(Self { waiting: BTreeMap::new(), writing: BTreeMap::new(), listing: BTreeMap::new(), next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<ReadFile> for Files {
    type From = Anyone;
    type Returns = Later<FileRead>;

    fn handle(&mut self, read: ReadFile, turn: &mut Turn<'_, Self>) -> Result<Later<FileRead>, Fault> {
        let (held, later) = turn.hold();
        let number = self.number();
        turn.run(&Load::new(read.namespace, read.path), &Reading { number })?;
        self.waiting.insert(number, held);

        Ok(later)
    }
}

impl Handles<WriteFile> for Files {
    type From = Anyone;
    type Returns = Reply<FileWritten>;

    fn handle(&mut self, write: WriteFile, turn: &mut Turn<'_, Self>) -> Result<Reply<FileWritten>, Fault> {
        if !write.namespace.is_written() {
            return Ok(Reply::Now(FileWritten::ReadOnly));
        }
        let (held, later) = turn.hold();
        let number = self.number();
        turn.run(&Save::new(write.namespace, write.path, write.bytes, write.how), &Saving { number })?;
        self.writing.insert(number, held);

        Ok(Reply::Later(later))
    }
}

impl Receives<Answer<FileWritten, Saving>> for Files {
    fn handle(
        &mut self,
        (answered, saving): (Answered<FileWritten>, Saving),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(held) = self.writing.remove(&saving.number) else {
            return Ok(());
        };
        let written = match answered {
            Answered::Yes(written) => written,
            Answered::No(_) => FileWritten::Failed,
        };
        turn.answer(held, &written)?;

        Ok(())
    }
}

impl Handles<ListFiles> for Files {
    type From = Anyone;
    type Returns = Reply<FilesListed>;

    fn handle(&mut self, list: ListFiles, turn: &mut Turn<'_, Self>) -> Result<Reply<FilesListed>, Fault> {
        if !list.namespace.is_listed() {
            return Ok(Reply::Now(FilesListed::NotListed));
        }
        let (held, later) = turn.hold();
        let number = self.number();
        turn.run(&Scan::new(list.namespace), &Scanning { number })?;
        self.listing.insert(number, held);

        Ok(Reply::Later(later))
    }
}

impl Receives<Answer<FilesListed, Scanning>> for Files {
    fn handle(
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
}

impl Files {
    /// The number of the next job it runs, taken.
    const fn number(&mut self) -> u64 {
        let number = self.next;
        self.next += 1;

        number
    }
}

impl Receives<Answer<FileRead, Reading>> for Files {
    fn handle(
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
}
