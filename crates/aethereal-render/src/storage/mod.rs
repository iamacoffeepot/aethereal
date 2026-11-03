//! Storage: the bytes a program's read-only storage buffer is bound to, made and written by one actor, as a field of
//! splats or particles is written by what moves them (ADR-0023).

#[cfg(test)]
mod tests;

use aethereal_actor::Bytes;
use aethereal_data::{Bound, Plain};

/// Asks the renderer for storage holding `bytes`, whose size is fixed from then. Answered with [`Stored`].
#[derive(Bound)]
#[kind(name = "aether.render.create_storage")]
pub struct CreateStorage {
    pub bytes: Bytes,
}

/// Asks the renderer for storage of `bytes` bytes, every one zero, whose size is fixed from then: storage a compute
/// pass fills, made without its zeros crossing as a blob. Answered with [`Stored`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.create_zeroed_storage")]
pub struct CreateZeroedStorage {
    pub bytes: u64,
}

/// Writes `bytes` over the bytes of `storage` from the byte `first`. Only its maker may write it, and nothing is
/// answered: a write that does not fit is logged, and the bytes are left as they were.
#[derive(Bound)]
#[kind(name = "aether.render.write_storage")]
pub struct WriteStorage {
    pub storage: StorageId,
    pub first: u64,
    pub bytes: Bytes,
}

/// Storage the renderer made, which any actor handed it may bind or read back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.storage_id")]
pub struct StorageId {
    pub(crate) id: u64,
}

/// What the renderer answers [`CreateStorage`] with.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.stored")]
pub enum Stored {
    Made(StorageId),
    Refused(StorageRefused),
}

/// Why storage was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.storage_refused")]
pub enum StorageRefused {
    /// There are no bytes, or they are no whole number of four, or more than the `most` the GPU binds as one storage
    /// buffer.
    Size { bytes: u64, most: u64 },
}

/// Checks storage of `size` bytes before it is made: that they are whole words, and as many as the GPU binds as one
/// storage buffer, the `most`. Answers its size.
///
/// # Errors
///
/// Why it would not be made as asked.
pub fn check(size: u64, most: u64) -> Result<u64, StorageRefused> {
    let bound = size > 0 && size.is_multiple_of(4) && size <= most;
    if !bound {
        return Err(StorageRefused::Size { bytes: size, most });
    }

    Ok(size)
}

/// The bytes a draw reads its counts from, five words: its index count, instance count, first index, base vertex,
/// and first instance (ADR-0026).
pub(crate) const DRAW_ARGUMENTS: u64 = 20;

/// The bytes a dispatch reads its counts from, three words: its workgroups along each dimension (ADR-0026).
pub(crate) const DISPATCH_ARGUMENTS: u64 = 12;

/// Whether `bytes` from the byte `first` lie inside storage of `size` bytes, on whole words, as the GPU writes a
/// buffer and reads a draw's or a dispatch's arguments from one.
pub(crate) fn fits(size: u64, first: u64, bytes: u64) -> bool {
    let inside = first.checked_add(bytes).is_some_and(|end| end <= size);
    let aligned = first.is_multiple_of(4) && bytes.is_multiple_of(4);

    inside && aligned
}
