//! The job that prunes entries and writes records (ADR-0039).

use crate::record::{Names, Recorded};
use crate::stockroom::RecordFile;
use crate::{Stockroom, StoreHash, StoreName};
use aethereal_actor::{Fault, Job, Runner, Running};
use aethereal_data::{Plain, to_bytes};
use std::collections::BTreeMap;

/// Removes the entries `prune` names, then writes each of `records` and the names file. Private to this crate.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.shelve")]
pub struct Shelve {
    /// The entries to remove, bytes and record.
    pub prune: Vec<StoreHash>,
    /// The records to write.
    pub records: Vec<Recorded>,
    /// Every name, and its hash.
    pub names: BTreeMap<StoreName, StoreHash>,
}

/// What the store keeps for a write it runs: nothing, as it runs one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.shelving")]
pub struct Shelving;

/// What became of a [`Shelve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.shelved")]
pub enum Shelved {
    /// Every entry asked is removed, and every record written.
    Done,
    /// The host serves no stockroom, or could not remove or write all of it.
    Failed,
}

impl Job for Shelve {
    type Done = Shelved;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Shelved, Fault> {
        let Some(room) = job.service::<dyn Stockroom>() else {
            return Ok(Shelved::Failed);
        };
        let files: Result<Vec<RecordFile>, _> = self
            .records
            .iter()
            .map(|Recorded { hash, record }| to_bytes(record).map(|bytes| RecordFile { hash: *hash, bytes }))
            .collect();
        let (Ok(files), Ok(names)) = (files, to_bytes(&Names { names: self.names })) else {
            return Ok(Shelved::Failed);
        };
        let pruned = room.prune(self.prune).await;
        let written = room.write(files, names).await;
        let shelved = match (pruned, written) {
            (Ok(()), Ok(())) => Shelved::Done,
            _ => Shelved::Failed,
        };

        Ok(shelved)
    }
}
