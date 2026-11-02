//! What the store writes beside its entries, so it is rebuilt as it begins (ADR-0039).

use crate::{StoreHash, StoreName};
use aethereal_data::Plain;
use std::collections::BTreeMap;

/// The most bytes reading one record may allocate: a record holds no collection, so this is far more than it needs.
pub const RECORD_BUDGET_BYTES: usize = 1 << 10;
/// The most bytes reading the names file may allocate.
pub const NAMES_BUDGET_BYTES: usize = 1 << 26;

/// One entry's record, written beside its bytes as `<hash>.entry`. Uses are counted, not timed: each check-in and
/// each hold takes the store's next count, which a restart carries on from the highest a record holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.record")]
pub struct Record {
    /// How many bytes the entry holds.
    pub bytes: u64,
    /// The count it was last checked in at, which orders the store's listing.
    pub checked_in: u64,
    /// The count it was last checked in or held at, which orders its pruning.
    pub used: u64,
    /// Whether it is pinned.
    pub pinned: bool,
}

impl Record {
    /// The highest count it holds.
    pub fn last_count(&self) -> u64 {
        self.checked_in.max(self.used)
    }
}

/// The names file, `names`: every name, and the hash it points at.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.names")]
pub struct Names {
    /// Each name, and its hash.
    pub names: BTreeMap<StoreName, StoreHash>,
}

/// An entry and its record, as the store's jobs carry them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct Recorded {
    /// The entry.
    pub hash: StoreHash,
    /// Its record.
    pub record: Record,
}
