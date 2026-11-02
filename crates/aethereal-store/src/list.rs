//! Listing what the store holds (ADR-0039).

use crate::{StoreHash, StoreName};
use aethereal_data::Plain;

/// List every entry in the store: asked of [`Store`](crate::Store), and answered [`Stored`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.list_stored")]
pub struct ListStored;

/// Every entry in the store, the last checked in first.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.stored")]
pub struct Stored {
    /// The entries.
    pub entries: Vec<Entry>,
}

/// One binary in the store. Its bytes are never given back (ADR-0039).
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct Entry {
    /// The hash of its bytes.
    pub hash: StoreHash,
    /// The names that point at it, in order: none when every name has moved on.
    pub names: Vec<StoreName>,
    /// How many bytes it holds.
    pub bytes: u64,
    /// Whether it is pinned.
    pub pinned: bool,
    /// Whether an actor holds it.
    pub held: bool,
}
