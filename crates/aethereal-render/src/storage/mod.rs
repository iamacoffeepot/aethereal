//! Storage: the bytes a program's read-only storage buffer is bound to, made and written by one actor, as a field of
//! splats or particles is written by what moves them (ADR-0023).

#[cfg(test)]
mod tests;

use aethereal_actor::BlobRef;
use aethereal_data::{Bound, Kind, KindName, Plain};

/// Asks the renderer for storage holding `bytes`, whose size is fixed from then. Answered with [`Stored`].
#[derive(Bound)]
pub struct CreateStorage {
    pub bytes: BlobRef,
}

impl Kind for CreateStorage {
    const NAME: KindName = KindName::new("aether.render.create_storage");
}

/// Writes `bytes` over the bytes of `storage` from the byte `first`. Only its maker may write it, and nothing is
/// answered: a write that does not fit is logged, and the bytes are left as they were.
#[derive(Bound)]
pub struct WriteStorage {
    pub storage: StorageId,
    pub first: u32,
    pub bytes: BlobRef,
}

impl Kind for WriteStorage {
    const NAME: KindName = KindName::new("aether.render.write_storage");
}

/// Storage the renderer made, which any actor handed it may bind or read back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct StorageId {
    pub(crate) id: u64,
}

impl Kind for StorageId {
    const NAME: KindName = KindName::new("aether.render.storage_id");
}

/// What the renderer answers [`CreateStorage`] with.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub enum Stored {
    Made(StorageId),
    Refused(StorageRefused),
}

impl Kind for Stored {
    const NAME: KindName = KindName::new("aether.render.stored");
}

/// Why storage was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum StorageRefused {
    /// There are no bytes, or they are no whole number of four, or more than the `most` the GPU binds as one storage
    /// buffer.
    Size { bytes: u64, most: u64 },
}

impl Kind for StorageRefused {
    const NAME: KindName = KindName::new("aether.render.storage_refused");
}

/// Checks storage before it is made: that its bytes are whole words, and as many as the GPU binds as one storage
/// buffer, the `most`. Answers its size.
///
/// # Errors
///
/// Why it would not be made as asked.
pub fn check(bytes: &[u8], most: u64) -> Result<u32, StorageRefused> {
    let size = bytes.len() as u64;
    let bound = size > 0 && size.is_multiple_of(4) && size <= most;
    let (true, Ok(sized)) = (bound, u32::try_from(size)) else {
        return Err(StorageRefused::Size { bytes: size, most });
    };

    Ok(sized)
}

/// The bytes a draw reads its counts from, five words: its index count, instance count, first index, base vertex,
/// and first instance (ADR-0026).
pub(crate) const DRAW_ARGUMENTS: u64 = 20;

/// The bytes a dispatch reads its counts from, three words: its workgroups along each dimension (ADR-0026).
pub(crate) const DISPATCH_ARGUMENTS: u64 = 12;

/// Whether `bytes` from the byte `first` lie inside storage of `size` bytes, on whole words, as the GPU writes a
/// buffer and reads a draw's or a dispatch's arguments from one.
pub(crate) fn fits(size: u32, first: u32, bytes: u64) -> bool {
    let end = u64::from(first) + bytes;
    let inside = end <= u64::from(size);
    let aligned = first.is_multiple_of(4) && bytes.is_multiple_of(4);

    inside && aligned
}
