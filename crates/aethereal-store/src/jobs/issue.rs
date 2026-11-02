//! The job that copies an entry to a launch copy, a file to run (ADR-0040).

use crate::{Stockroom, StoreHash};
use aethereal_actor::{Fault, Job, Runner, Running};
use aethereal_data::Plain;

/// Copies the entry `hash` to the launch copy `copy`, marked executable. Private to this crate, so no actor but
/// [`Store`](crate::Store) makes one.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.issue")]
pub struct Issue {
    /// The entry.
    pub hash: StoreHash,
    /// The launch copy's file name, unique among those the store has not removed.
    pub copy: String,
}

/// What the store keeps for a launch copy it makes: nothing, as it runs one job at a time and keeps the launch it is
/// for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.issuing")]
pub struct Issuing;

/// What became of an [`Issue`].
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.issued")]
pub enum Issued {
    /// The launch copy is made.
    Ready {
        /// Its absolute path on the host's machine.
        program: String,
    },
    /// The host serves no stockroom, or could not make the copy.
    Failed,
}

impl Job for Issue {
    type Done = Issued;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Issued, Fault> {
        let Some(room) = job.service::<dyn Stockroom>() else {
            return Ok(Issued::Failed);
        };
        let issued = room.launch(self.hash, self.copy).await;

        Ok(issued.map_or(Issued::Failed, |program| Issued::Ready { program }))
    }
}
