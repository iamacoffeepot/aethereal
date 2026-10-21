//! Instances: the records a mesh is drawn once for each of, read by a program's vertex stage once per instance, made
//! and written by one actor (ADR-0023).

#[cfg(test)]
mod tests;

use crate::mesh::{self, MeshRefused, VertexLayout};
use aethereal_actor::BlobRef;
use aethereal_data::{Bound, Kind, KindName, Plain};

/// Asks the renderer for instances: room for `capacity` records laid out as `layout` says, holding `records` to begin
/// with. Answered with [`Instanced`].
#[derive(Bound)]
pub struct CreateInstances {
    pub layout: VertexLayout,
    pub capacity: u32,
    pub records: BlobRef,
}

impl Kind for CreateInstances {
    const NAME: KindName = KindName::new("aether.render.create_instances");
}

/// Writes `records` over the records of `instances` from the `first`: one that passes their end extends them, and an
/// empty one cuts them to their first `first`. Only their maker may write them, and nothing is answered: a write that
/// does not fit is logged, and the records are left as they were.
#[derive(Bound)]
pub struct WriteInstances {
    pub instances: InstancesId,
    pub first: u32,
    pub records: BlobRef,
}

impl Kind for WriteInstances {
    const NAME: KindName = KindName::new("aether.render.write_instances");
}

/// Instances the renderer made, which any actor handed them may draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct InstancesId {
    pub(crate) id: u64,
}

impl Kind for InstancesId {
    const NAME: KindName = KindName::new("aether.render.instances_id");
}

/// What the renderer answers [`CreateInstances`] with.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub enum Instanced {
    Made(InstancesId),
    Refused(InstancesRefused),
}

impl Kind for Instanced {
    const NAME: KindName = KindName::new("aether.render.instanced");
}

/// Why instances were not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum InstancesRefused {
    /// The layout is one no record could have.
    Layout { why: MeshRefused },
    /// There is room for no record, or for more bytes than the GPU puts in a buffer.
    Capacity { capacity: u32 },
    /// The records are no whole number of the stride, or more than there is room for.
    Records { bytes: u64 },
}

impl Kind for InstancesRefused {
    const NAME: KindName = KindName::new("aether.render.instances_refused");
}

/// Checks instances before they are made: their layout, their room within the `most` bytes the GPU puts in a buffer,
/// and that their records are whole and fit. Answers how many records they hold.
///
/// # Errors
///
/// Why they would not be made as asked.
pub fn check(layout: &VertexLayout, capacity: u32, records: &[u8], most: u64) -> Result<u32, InstancesRefused> {
    mesh::check_layout(layout).map_err(|why| InstancesRefused::Layout { why })?;

    let stride = u64::from(layout.stride);
    let room = u64::from(capacity) * stride;
    let roomy = capacity > 0 && room <= most;
    if !roomy {
        return Err(InstancesRefused::Capacity { capacity });
    }

    let bytes = records.len() as u64;
    let whole = bytes.is_multiple_of(stride);
    let fits = bytes <= room;
    let counted = u32::try_from(bytes / stride);
    let (true, true, Ok(length)) = (whole, fits, counted) else {
        return Err(InstancesRefused::Records { bytes });
    };

    Ok(length)
}

/// How instances stand: the bytes of a record, the records there is room for, and how many they hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Held {
    pub stride: u32,
    pub capacity: u32,
    pub length: u32,
}

/// Why a write to instances was left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteRefused {
    /// The write begins past the records they hold.
    First { first: u32, length: u32 },
    /// The write is no whole number of records, or passes their room.
    Records { bytes: u64 },
}

impl Held {
    /// What instances hold once `bytes` of records are written from the `first`: as many records as the write
    /// reaches or they held before, or the `first` when the write is empty.
    pub fn write(self, first: u32, bytes: u64) -> Result<u32, WriteRefused> {
        if first > self.length {
            return Err(WriteRefused::First { first, length: self.length });
        }
        let stride = u64::from(self.stride);
        let whole = bytes.is_multiple_of(stride);
        let end = u64::from(first) + bytes / stride;
        let fits = end <= u64::from(self.capacity);
        let (true, true, Ok(end)) = (whole, fits, u32::try_from(end)) else {
            return Err(WriteRefused::Records { bytes });
        };
        if bytes == 0 {
            return Ok(first);
        }

        Ok(self.length.max(end))
    }
}
