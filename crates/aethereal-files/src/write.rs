//! What an actor asks the file capability to write, and what it is answered (ADR-0039).

use crate::Namespace;
use aethereal_actor::Bytes;
use aethereal_data::{Bound, FilePath, Plain};

/// Write `bytes` as the whole file at `path` in `namespace`: asked of [`Files`](crate::Files), and answered a
/// [`FileWritten`].
#[derive(Debug, PartialEq, Eq, Bound)]
#[kind(name = "aether.files.write_file")]
pub struct WriteFile {
    /// The namespace the file is in.
    pub namespace: Namespace,
    /// Where it is in the namespace.
    pub path: FilePath,
    /// The whole file, in a blob (ADR-0042).
    pub bytes: Bytes,
}

/// What became of a [`WriteFile`]: written, or why not. Each host answers in it the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.files.file_written")]
pub enum FileWritten {
    /// The file is written whole.
    Written,
    /// The namespace is not written, as `Assets` is not. Answered in the turn that asks, with no job run.
    ReadOnly,
    /// The host has nowhere to write it.
    Unavailable,
    /// The host could not write it.
    Failed,
}
