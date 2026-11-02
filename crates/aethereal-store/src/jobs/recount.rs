//! The job that reads what the stockroom keeps, as the store begins (ADR-0039).

use crate::record::{NAMES_BUDGET_BYTES, Names, RECORD_BUDGET_BYTES, Record, Recorded};
use crate::stockroom::{NamesFile, RecordFile, Shelf};
use crate::{Stockroom, StoreHash, StoreName};
use aethereal_actor::{Fault, Job, Runner, Running};
use aethereal_data::{Budget, Plain, from_bytes};
use std::collections::BTreeMap;

/// Reads every record and the names file, and removes each entry whose record does not read. Private to this crate,
/// as each of the store's jobs is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.recount")]
pub struct Recount;

/// What the store keeps for its recount: nothing, as it runs one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.counting")]
pub struct Counting;

/// What a [`Recount`] found.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.counted")]
pub enum Counted {
    /// Every entry whose record reads, and every name.
    Read {
        /// The entries, in no order.
        entries: Vec<Recorded>,
        /// The names, which may point at an entry no longer kept.
        names: BTreeMap<StoreName, StoreHash>,
    },
    /// The host serves no stockroom, could not read it, or its names file does not read.
    Failed,
}

impl Job for Recount {
    type Done = Counted;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Counted, Fault> {
        let Some(room) = job.service::<dyn Stockroom>() else {
            return Ok(Counted::Failed);
        };
        let Ok(Shelf { records, names }) = room.count().await else {
            return Ok(Counted::Failed);
        };
        let names = match names {
            NamesFile::Absent => BTreeMap::new(),
            NamesFile::Read(bytes) => match from_bytes::<Names>(&bytes, &mut Budget::new(NAMES_BUDGET_BYTES)) {
                Ok(Names { names }) => names,
                Err(_) => return Ok(Counted::Failed),
            },
        };
        let (entries, unread) = read(records);
        // An entry left unread is not indexed, so one whose removal fails is only tried again at the next recount.
        let _ = room.prune(unread).await;

        Ok(Counted::Read { entries, names })
    }
}

/// Each entry whose record reads, with its record, and the hash of each whose record does not.
fn read(records: Vec<RecordFile>) -> (Vec<Recorded>, Vec<StoreHash>) {
    let mut entries = Vec::with_capacity(records.len());
    let mut unread = Vec::new();
    for RecordFile { hash, bytes } in records {
        match from_bytes::<Record>(&bytes, &mut Budget::new(RECORD_BUDGET_BYTES)) {
            Ok(record) => entries.push(Recorded { hash, record }),
            Err(_) => unread.push(hash),
        }
    }

    (entries, unread)
}
