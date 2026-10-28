//! What an actor asks the file capability, and what it is answered (ADR-0032).

use crate::Namespace;
use aethereal_actor::BlobRef;
use aethereal_data::{Bound, FilePath, Plain};

/// Read the whole file at `path` in `namespace`: asked of [`Files`](crate::Files), and answered a [`FileRead`].
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.files.read_file")]
pub struct ReadFile {
    /// The namespace the file is in.
    pub namespace: Namespace,
    /// Where it is in the namespace.
    pub path: FilePath,
}

/// What became of a [`ReadFile`]: the file, or why it was not read. Each host answers in it the same way.
#[derive(Bound)]
#[kind(name = "aether.files.file_read")]
pub enum FileRead {
    /// The whole file, as a blob (ADR-0014).
    Read {
        /// The file's bytes.
        bytes: BlobRef,
    },
    /// No file is at that path.
    Missing,
    /// The host has nowhere to read it from.
    Unavailable,
    /// The host could not read it, or it was larger than the engine's budget for blobs.
    Failed,
}
