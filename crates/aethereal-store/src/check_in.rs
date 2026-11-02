//! Checking a binary in, and what became of it (ADR-0039).

use crate::{StoreHash, StoreName};
use aethereal_data::Plain;

/// Copy the file at `path` on the host's machine into the store, and point `name` at it: asked of
/// [`Store`](crate::Store), and answered a [`CheckedIn`].
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.check_in")]
pub struct CheckIn {
    /// Where the file is on the host's machine: relative to where the program runs, or absolute.
    pub path: String,
    /// The name that points at it once it is in.
    pub name: StoreName,
}

/// What became of a [`CheckIn`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.checked_in")]
pub enum CheckedIn {
    /// The file is in the store, and its name points at it.
    Stored {
        /// The hash of its bytes.
        hash: StoreHash,
        /// How many bytes it holds.
        bytes: u64,
        /// Whether these bytes were not already in the store.
        fresh: bool,
    },
    /// Nothing is at the path.
    Missing,
    /// What is at the path is not a regular file: a directory, a device, or a pipe.
    NotAFile,
    /// The file holds more than the store's `most_file_bytes`.
    TooLarge,
    /// The file does not fit within the store's `most_bytes`, even after pruning all it may.
    Full,
    /// The host could not read the file or write the store.
    Failed,
}
