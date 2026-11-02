//! Launching a stored binary: a copy of it to run, its build held while the asker lives (ADR-0040).

use crate::{StoreHash, StoreName};
use aethereal_data::Plain;

/// Copy the entry `binary` names to a launch copy, a file to run, and hold the entry for as long as the asker lives:
/// asked of [`Store`](crate::Store), and answered [`Launched`]. The hold is a [`Hold`](crate::Hold)'s, and counts as the
/// entry's use. The launch copy is removed when the asker ends, and every one is removed as the store begins.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.launch")]
pub struct Launch {
    /// The binary to launch.
    pub binary: Binary,
}

/// A stored binary, as an asker names it: by a name pointing at it, or by its hash. A name never spells a hash, so the
/// one string an agent writes for a binary is a hash when it spells one, and a name otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub enum Binary {
    /// The entry the name points at.
    Named(StoreName),
    /// The entry of the hash.
    Hash(StoreHash),
}

/// What became of a [`Launch`].
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.launched")]
pub enum Launched {
    /// The launch copy is made, and the entry held until the asker ends.
    Ready {
        /// The launch copy's absolute path on the host's machine: `running/<name>-<n>` beneath the store's directory,
        /// marked executable. `<name>` is the name the launch asked by; for a hash, the first name pointing at it in
        /// the order of their bytes, or the hash's first 12 hexadecimal digits when none does. `<n>` is the store's
        /// count of launches since it began.
        program: String,
        /// The hash of the entry it copies.
        hash: StoreHash,
    },
    /// The store holds no entry of that name or hash, so nothing is held.
    NotStored,
    /// The host could not make the launch copy. The entry is held until the asker ends all the same.
    Failed,
}
