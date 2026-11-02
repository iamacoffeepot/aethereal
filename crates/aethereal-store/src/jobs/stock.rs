//! The job that copies a file into the store (ADR-0039).

use crate::stockroom::{Copied, Uncopied};
use crate::{Stockroom, StoreHash};
use aethereal_actor::{Fault, Job, Runner, Running};
use aethereal_data::Plain;

/// Copies the file at `path` into the stockroom, hashing it as it copies. The bytes never enter the engine. Private
/// to this crate, so no actor but [`Store`](crate::Store) copies a file in.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.stock")]
pub struct Stock {
    /// Where the file is on the host's machine.
    pub path: String,
    /// The most bytes it may hold.
    pub most_file_bytes: u64,
}

/// What the store keeps for a copy it runs: nothing, as it runs one at a time and keeps the check-in it is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.stocking")]
pub struct Stocking;

/// What became of a [`Stock`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.stocked")]
pub enum Stocked {
    /// The file is in the stockroom.
    Copied {
        /// The hash of its bytes.
        hash: StoreHash,
        /// How many bytes it holds.
        bytes: u64,
    },
    /// Nothing is at the path.
    Missing,
    /// What is at the path is not a regular file.
    NotAFile,
    /// The file holds more bytes than it may.
    TooLarge,
    /// The host serves no stockroom, or could not read the file or write its copy.
    Failed,
}

impl Job for Stock {
    type Done = Stocked;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Stocked, Fault> {
        let Some(room) = job.service::<dyn Stockroom>() else {
            return Ok(Stocked::Failed);
        };
        let copied = room.copy(self.path, self.most_file_bytes).await;
        let stocked = match copied {
            Ok(Copied { hash, bytes }) => Stocked::Copied { hash, bytes },
            Err(Uncopied::Missing) => Stocked::Missing,
            Err(Uncopied::NotAFile) => Stocked::NotAFile,
            Err(Uncopied::TooLarge) => Stocked::TooLarge,
            Err(Uncopied::Failed) => Stocked::Failed,
        };

        Ok(stocked)
    }
}
