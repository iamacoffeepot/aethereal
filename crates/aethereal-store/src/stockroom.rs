//! The host's stockroom, where the store's bytes and records are kept: a service the host serves its jobs (ADR-0034,
//! ADR-0039).

use crate::StoreHash;
use std::future::Future;
use std::pin::Pin;

/// A file copied into the stockroom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Copied {
    /// The hash of its bytes, which names its entry.
    pub hash: StoreHash,
    /// How many bytes it holds.
    pub bytes: u64,
}

/// Why a file was not copied in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uncopied {
    /// Nothing is at the path.
    Missing,
    /// What is at the path is not a regular file.
    NotAFile,
    /// The file holds more bytes than it may.
    TooLarge,
    /// The host could not read it or write its copy.
    Failed,
}

/// The record of one entry, as the store wrote it: the stockroom keeps its bytes beside the entry's and never reads
/// them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordFile {
    /// The entry it records.
    pub hash: StoreHash,
    /// What the store wrote.
    pub bytes: Vec<u8>,
}

/// The names file, as the stockroom found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamesFile {
    /// None was ever written.
    Absent,
    /// What the store last wrote.
    Read(Vec<u8>),
}

/// Everything the stockroom keeps but the bytes of its entries, as it is found when the store begins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shelf {
    /// The record of each entry whose bytes are kept beside it, in no order.
    pub records: Vec<RecordFile>,
    /// The names file.
    pub names: NamesFile,
}

/// The host could not do what was asked of the stockroom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unkept;

/// A copy into the stockroom, as it does it.
pub type Copying<'a> = Pin<Box<dyn Future<Output = Result<Copied, Uncopied>> + 'a>>;
/// A count of what the stockroom keeps, as it does it.
pub type Recounting<'a> = Pin<Box<dyn Future<Output = Result<Shelf, Unkept>> + 'a>>;
/// A write or a removal in the stockroom, as it does it.
pub type Keeping<'a> = Pin<Box<dyn Future<Output = Result<(), Unkept>> + 'a>>;

/// The host's stockroom, where the store keeps each entry's bytes, its record, and the names file (ADR-0039): a
/// directory natively. A host serves it into the services its runners hand every job (ADR-0034); a browser serves
/// none, and bootstraps no store.
///
/// Nothing it does blocks the thread that polls it: a host whose disk blocks works on threads of its own, and wakes
/// what it was asked once it is done. The store asks one thing of it at a time, so what it is asked is done in order.
pub trait Stockroom: Send + Sync {
    /// Copies the file at `path` in, hashing it as it copies: only a regular file of at most `most_bytes`. A copy of
    /// bytes already kept is let go, and the entry kept.
    fn copy(&self, path: String, most_bytes: u64) -> Copying<'_>;

    /// Reads every record and the names file. On the way it removes what a check-in or a write cut short left: an
    /// entry without its record, a record without its entry, and a copy or a write never finished.
    fn count(&self) -> Recounting<'_>;

    /// Writes each of `records` and then the names file, each whole: each is written beside itself and renamed over
    /// what it replaces, so a crash leaves the old or the new.
    fn write(&self, records: Vec<RecordFile>, names: Vec<u8>) -> Keeping<'_>;

    /// Removes the entries `hashes` name, their bytes and their records. One already gone is not missed.
    fn prune(&self, hashes: Vec<StoreHash>) -> Keeping<'_>;
}
