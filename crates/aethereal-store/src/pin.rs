//! Pinning an entry, so it is never pruned (ADR-0039).

use crate::StoreHash;
use aethereal_data::Plain;

/// Keep the entry `hash` from being pruned: asked of [`Store`](crate::Store), and answered [`Pinned`]. Pinning an entry
/// already pinned changes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.pin")]
pub struct Pin {
    /// The entry.
    pub hash: StoreHash,
}

/// Clear the pin of the entry `hash`: asked of [`Store`](crate::Store), and answered [`Pinned`]. Its names and holds
/// still keep it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.unpin")]
pub struct Unpin {
    /// The entry.
    pub hash: StoreHash,
}

/// What became of a [`Pin`] or an [`Unpin`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.pinned")]
pub enum Pinned {
    /// The entry is pinned, or no longer, as asked.
    Done,
    /// The store holds no entry of that hash.
    NotStored,
}
