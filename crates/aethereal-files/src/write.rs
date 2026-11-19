//! What an actor asks the file capability to write, and what it is answered (ADR-0039).

use crate::Namespace;
use aethereal_actor::Bytes;
use aethereal_data::{Bound, FilePath, Plain};

/// Write `bytes` to the file at `path` in `namespace`, as `how` says: asked of [`Files`](crate::Files), and answered a
/// [`FileWritten`].
#[derive(Debug, PartialEq, Eq, Bound)]
#[kind(name = "aether.files.write_file")]
pub struct WriteFile {
    /// The namespace the file is in.
    pub namespace: Namespace,
    /// Where it is in the namespace.
    pub path: FilePath,
    /// What is written, in a blob (ADR-0042).
    pub bytes: Bytes,
    pub how: Writes,
}

/// How a write lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.files.writes")]
pub enum Writes {
    /// As the whole file: a reader sees it as it was, or all of it.
    Whole,
    /// After what the file holds, made when missing, and synced before it is answered.
    ///
    /// An append is not all or nothing: a crash can leave a cut-off tail, so whoever appends frames what it writes
    /// and reads back only whole frames. Appends to one file land in the order they were asked, as every write of one
    /// file does.
    Append,
}

/// What became of a [`WriteFile`]: written, or why not. Each host answers in it the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.files.file_written")]
pub enum FileWritten {
    /// The file is written as asked: whole, or appended to.
    Written,
    /// The namespace is not written, as `Assets` is not. Answered in the turn that asks, with no job run.
    ReadOnly,
    /// The host has nowhere to write it.
    Unavailable,
    /// The host could not write it.
    Failed,
}
