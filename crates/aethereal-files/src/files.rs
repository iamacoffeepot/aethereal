//! The file capability's root (ADR-0032, ADR-0039).

use crate::load::Load;
use crate::save::Save;
use crate::{FileRead, FileWritten, ReadFile, WriteFile};
use aethereal_actor::{
    Actor, Answer, Answered, Beginning, Carries, ChildOf, Ending, Fault, Handles, Held, ToSender, Top, Turn,
};
use aethereal_data::{Bound, Plain};
use std::collections::BTreeMap;

/// The file capability: reads and writes files for every other actor, in named namespaces, by path (ADR-0032,
/// ADR-0039). An actor asks it a [`ReadFile`] or a [`WriteFile`] at its typed path, `path::<Files>(())`, and is
/// answered a [`FileRead`] or a [`FileWritten`] once the read or the write is done.
///
/// It holds each request, runs a read or a write for it, and answers it with what
/// that came to; one the host could not run is answered `Failed`. A write to a
/// namespace that is not written is answered [`FileWritten::ReadOnly`] in the turn
/// that asks, with no job run.
#[derive(Bound)]
#[kind(name = "aether.files")]
pub struct Files {
    /// The reads it holds, each by the number of the job it ran for it.
    waiting: BTreeMap<u64, Held<FileRead>>,
    /// The writes it holds, each by the number of the job it ran for it.
    writing: BTreeMap<u64, Held<FileWritten>>,
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

impl ChildOf for Files {
    type Parent = Top;
}

/// Asked at its typed path, it answers a read with what became of it (ADR-0022).
impl Carries<ReadFile> for Files {
    type Answers = (FileRead,);
}

/// Asked at its typed path, it answers a write with what became of it.
impl Carries<WriteFile> for Files {
    type Answers = (FileWritten,);
}

impl Actor for Files {
    type Config = FilesConfig;
    type Key = ();
    type Handles = (ReadFile, Answer<FileRead, Reading>, WriteFile, Answer<FileWritten, Saving>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: FilesConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { waiting: BTreeMap::new(), writing: BTreeMap::new(), next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<ReadFile> for Files {
    type Sends = (ToSender<FileRead>,);

    fn handle(&mut self, read: ReadFile, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = turn.hold()?;
        let number = self.number();
        turn.run(&Load::new(read.namespace, read.path), &Reading { number })?;
        self.waiting.insert(number, held);

        Ok(())
    }
}

impl Handles<WriteFile> for Files {
    type Sends = (ToSender<FileWritten>,);

    fn handle(&mut self, write: WriteFile, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if !write.namespace.is_written() {
            turn.reply(&FileWritten::ReadOnly)?;
            return Ok(());
        }
        let held = turn.hold()?;
        let number = self.number();
        turn.run(&Save::new(write.namespace, write.path, write.bytes), &Saving { number })?;
        self.writing.insert(number, held);

        Ok(())
    }
}

impl Handles<Answer<FileWritten, Saving>> for Files {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, saving): (Answered<FileWritten>, Saving),
        turn: &mut Turn<'_, Self, ()>,
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

impl Files {
    /// The number of the next job it runs, taken.
    const fn number(&mut self) -> u64 {
        let number = self.next;
        self.next += 1;

        number
    }
}

impl Handles<Answer<FileRead, Reading>> for Files {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, reading): (Answered<FileRead>, Reading),
        turn: &mut Turn<'_, Self, ()>,
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
