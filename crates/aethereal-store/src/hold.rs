//! Holding an entry while its holder lives (ADR-0039).

use crate::StoreHash;
use aethereal_data::Plain;

/// Keep the entry `hash` from being pruned for as long as the asker lives: asked of [`Store`](crate::Store), and
/// answered [`Held`]. The hold goes when the asker ends, and is never written to disk. An actor may hold several
/// entries; holding one it holds already changes nothing, but that it counts as the entry's use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.hold")]
pub struct Hold {
    /// The entry.
    pub hash: StoreHash,
}

/// What became of a [`Hold`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.held")]
pub enum Held {
    /// The entry is held until the asker ends.
    Done,
    /// The store holds no entry of that hash, so nothing is held.
    NotStored,
}
