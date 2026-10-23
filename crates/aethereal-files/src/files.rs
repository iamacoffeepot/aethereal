//! The file capability's root (ADR-0032).

use crate::load::Load;
use crate::{FileRead, ReadFile};
use aethereal_actor::{
    Actor, Answer, Answered, Beginning, Carries, ChildOf, Ending, Fault, Handles, Held, ToSender, Top, Turn,
};
use aethereal_data::{Bound, Plain};
use std::collections::BTreeMap;

/// The file capability: reads files for every other actor, in named namespaces, by path (ADR-0032). An actor asks it a
/// [`ReadFile`] at its typed path, `path::<Files>(())`, and is answered a [`FileRead`] once the read is done.
///
/// It holds each request, runs a read for it, and answers it with what the read
/// came to; a read the host could not run is answered [`FileRead::Failed`].
#[derive(Bound)]
#[kind(name = "aether.files")]
pub struct Files {
    /// The requests it holds, each by the number of the read it ran for it.
    waiting: BTreeMap<u64, Held<FileRead>>,
    /// The number of the next read it runs.
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

impl ChildOf for Files {
    type Parent = Top;
}

/// Asked at its typed path, it answers a read with what became of it (ADR-0022).
impl Carries<ReadFile> for Files {
    type Answers = (FileRead,);
}

impl Actor for Files {
    type Config = FilesConfig;
    type Key = ();
    type Handles = (ReadFile, Answer<FileRead, Reading>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: FilesConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { waiting: BTreeMap::new(), next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<ReadFile> for Files {
    type Sends = (ToSender<FileRead>,);

    fn handle(&mut self, read: ReadFile, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = turn.hold()?;
        let number = self.next;
        self.next += 1;
        turn.run(&Load::new(read.namespace, read.path), &Reading { number })?;
        self.waiting.insert(number, held);

        Ok(())
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
