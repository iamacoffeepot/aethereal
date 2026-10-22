//! Dispatches: one compute program, run in one compute pass of a frame, with what it binds, over a count of
//! invocations, asked for by any actor and checked as it is made, as a placement is, so running it cannot fail
//! (ADR-0026).

#[cfg(test)]
mod tests;

use crate::compute::{Array, ComputeId};
use crate::frame::{self, Frame};
use crate::pass::{Pass, PassName};
use crate::placement::{self, Bind, Finds, JoinRefused, Resource};
use crate::program::{Access, Binding, BindingKind};
use crate::storage::{self, DISPATCH_ARGUMENTS, StorageId};
use aethereal_data::{ActorId, Plain};

/// Asks the renderer to run the compute program `join` names in the compute pass it names, with what it binds, over
/// `size`, at every `Render` until it is released. Answered with [`Dispatched`].
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.create_dispatch")]
pub struct CreateDispatch {
    pub join: Dispatch,
    pub size: Invocations,
}

/// The compute pass a dispatch runs in: the pass `pass` of `frame`, running `compute`, binding what `bindings` names at
/// each place the compute program binds.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.dispatch")]
pub struct Dispatch {
    pub frame: Frame,
    pub pass: PassName,
    pub compute: ComputeId,
    pub bindings: Vec<Bind>,
}

impl Dispatch {
    /// The compute pass `P` of `frame`, running `compute`, binding `bindings`: the pass named by its type.
    #[must_use]
    pub const fn to<P: Pass>(frame: Frame, compute: ComputeId, bindings: Vec<Bind>) -> Self {
        Self { frame, pass: PassName::of::<P>(), compute, bindings }
    }
}

/// How many invocations a dispatch runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.invocations")]
pub enum Invocations {
    /// One for each element of the array the storage ends in, which the dispatch binds, in the first dimension: the
    /// workgroups that cover them, the last one's invocations past the end reading `arrayLength` and stopping.
    PerElement(StorageId),
    /// A grid of this many workgroups along each dimension, each of the shader's size.
    Workgroups([u32; 3]),
    /// A grid of as many workgroups along each dimension as the three words of `storage` from the byte `at` hold at
    /// each frame, as compute or the storage's maker wrote them (ADR-0026).
    Indirect { storage: StorageId, at: u32 },
}

impl Invocations {
    /// The storage it reads its workgroups from: none for a dispatch for each element or of a grid given.
    pub(crate) const fn arguments(self) -> Option<StorageId> {
        match self {
            Self::Indirect { storage, .. } => Some(storage),
            Self::PerElement(_) | Self::Workgroups(_) => None,
        }
    }
}

/// What a dispatch runs, as its check found it: a grid of workgroups, worked out or given, or as many as the three
/// words of `storage` from the byte `at` hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Runs {
    Workgroups([u32; 3]),
    Indirect { storage: StorageId, at: u32 },
}

/// A dispatch the renderer made, which only its maker may release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.dispatch_id")]
pub struct DispatchId {
    pub(crate) id: u64,
}

/// What the renderer answers [`CreateDispatch`] with.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.dispatched")]
pub enum Dispatched {
    Made(DispatchId),
    Refused(DispatchRefused),
}

/// Why a dispatch was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.dispatch_refused")]
pub enum DispatchRefused {
    /// No frame is kept as the one named.
    Frame,
    /// The frame has no compute pass of the name: none of the name, or one that draws.
    Pass,
    /// No compute program is kept under the id: never made, or released.
    Compute,
    /// What it binds is not what the compute program binds.
    Bindings { why: JoinRefused },
    /// It binds storage another actor made at a place the compute program writes: only a storage's maker writes it.
    NotOwner { group: u32, binding: u32 },
    /// It binds storage at a place the compute program writes, and binds the same storage at another place or reads
    /// its workgroups from it, which WebGPU refuses in one dispatch.
    Aliased { group: u32, binding: u32 },
    /// It runs one invocation for each element of storage it does not bind, or binds at a place whose shader type
    /// ends in no array whose length is its buffer's.
    Elements,
    /// It runs more workgroups along a dimension than the GPU dispatches along one, `most`.
    Size { workgroups: [u32; 3], most: u32 },
    /// No storage is kept under the id it reads its workgroups from.
    Storage,
    /// Its workgroups, three words from the byte `at`, lie off a word or past the storage's `bytes`.
    Arguments { at: u32, bytes: u32 },
    /// It writes storage another dispatch of its pass reads or writes, or reads storage one of its pass writes, as the
    /// dispatches of one pass run in no order; or it writes storage a dispatch of another frame writes, and no order of
    /// frames runs one before the other.
    Written,
    /// It binds the texture of a frame, or storage a frame writes, that reads the frame it joins, through frames
    /// between or at once, or has that frame read storage it writes: frames would read each other round.
    Round,
}

/// One use of storage by what the renderer keeps: read in a pass of a frame by a placement, an effect, or a dispatch,
/// or written there by a dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Use {
    pub frame: Frame,
    pub pass: PassName,
    pub access: Access,
}

impl Use {
    /// The pair of a frame that ends on a texture and a frame it reads, by their ids, that two uses of one storage
    /// make: when one reads what the other writes, in other frames that each end on a texture. None when they make
    /// none: a frame that ends on a window runs after every frame that ends on a texture, which reads what it writes
    /// as it was written at the last `Render`, and passes of one frame are ordered by the frame.
    pub fn reads_from(self, other: Self) -> Option<(u64, u64)> {
        let (reader, writer) = match (self.access, other.access) {
            (Access::Read, Access::Write) => (self, other),
            (Access::Write, Access::Read) => (other, self),
            (Access::Read, Access::Read) | (Access::Write, Access::Write) => return None,
        };
        let (Frame::Texture(reader), Frame::Texture(writer)) = (reader.frame, writer.frame) else {
            return None;
        };

        (reader != writer).then_some((reader.id, writer.id))
    }

    /// Whether the two run in one pass of one frame.
    fn same_pass(self, other: Self) -> bool {
        self.frame == other.frame && self.pass == other.pass
    }
}

/// What a dispatch is checked against of the compute program it runs: what it binds, the size of its workgroups, and
/// the storage it binds that ends in an array whose length is its buffer's.
pub(crate) struct Computes<'a> {
    pub bindings: &'a [Binding],
    pub workgroup: [u32; 3],
    pub arrays: &'a [Array],
}

/// What the renderer keeps, as a dispatch's check looks it up, beside what a placement's check does.
pub(crate) trait Dispatches: Finds {
    /// Checks that the pass `pass` of `frame` as it is now is a compute pass: refused when there is no such frame, or
    /// it has no compute pass of the name.
    fn computes(&self, frame: Frame, pass: PassName) -> Result<(), DispatchRefused>;
    /// The compute program kept under `id`: none when none is.
    fn compute(&self, id: ComputeId) -> Option<Computes<'_>>;
    /// The actor that made the storage kept under `id`: none when none is.
    fn maker(&self, id: StorageId) -> Option<ActorId>;
    /// The most workgroups the GPU dispatches along one dimension.
    fn most_workgroups(&self) -> u32;
}

/// Checks a dispatch `by` asks for before it is made: that its pass is a compute pass of its frame, that its compute
/// program is kept and bound as it binds, only storage `by` made where it writes, and nothing it writes twice, that it
/// runs no more workgroups than the GPU dispatches, or reads them from storage they fit in, and that no two
/// dispatches write one storage in no order, and no frames read each other round. Answers what it runs.
pub(crate) fn check(create: &CreateDispatch, by: ActorId, kept: &impl Dispatches) -> Result<Runs, DispatchRefused> {
    let join = &create.join;
    kept.computes(join.frame, join.pass)?;
    let compute = kept.compute(join.compute).ok_or(DispatchRefused::Compute)?;
    placement::check_bindings(&join.bindings, compute.bindings, kept)
        .map_err(|why| DispatchRefused::Bindings { why })?;
    let stored = stored(&join.bindings, compute.bindings);
    // Workgroups read from storage are a read of it in the dispatch's pass, as storage it binds to read is.
    let counted = create.size.arguments().map(|storage| (storage, Access::Read));
    let accessed: Vec<(StorageId, Access)> =
        stored.iter().map(|(_, storage, access)| (*storage, *access)).chain(counted).collect();
    for (bind, storage, access) in &stored {
        if *access == Access::Read {
            continue;
        }
        let (group, binding) = (bind.group, bind.binding);
        let mine = kept.maker(*storage) == Some(by);
        if !mine {
            return Err(DispatchRefused::NotOwner { group, binding });
        }
        let aliased = accessed.iter().filter(|(other, _)| other == storage).count() > 1;
        if aliased {
            return Err(DispatchRefused::Aliased { group, binding });
        }
    }
    let runs = runs(create.size, &join.bindings, &compute, kept)?;
    if let Runs::Workgroups(workgroups) = runs {
        let most = kept.most_workgroups();
        let past = workgroups.iter().any(|along| *along > most);
        if past {
            return Err(DispatchRefused::Size { workgroups, most });
        }
    }
    check_order(join, &accessed, kept)?;

    Ok(runs)
}

/// Each storage `bindings` binds, beside the bind and how the compute program, which binds what `listed` lists, uses
/// it there.
fn stored<'a>(bindings: &'a [Bind], listed: &[Binding]) -> Vec<(&'a Bind, StorageId, Access)> {
    bindings
        .iter()
        .filter_map(|bind| {
            let Resource::Storage(storage) = bind.to else {
                return None;
            };
            let found = listed.iter().find(|listed| listed.group == bind.group && listed.binding == bind.binding)?;
            let BindingKind::Storage { access, .. } = found.kind else {
                return None;
            };
            Some((bind, storage, access))
        })
        .collect()
}

/// What `size` runs of `compute`, binding `bindings`: workgroups as given, or those that cover one invocation for each
/// element of the storage named, along the first dimension, or as many as storage they fit in holds.
fn runs(
    size: Invocations,
    bindings: &[Bind],
    compute: &Computes<'_>,
    kept: &impl Finds,
) -> Result<Runs, DispatchRefused> {
    let storage = match size {
        Invocations::Workgroups(workgroups) => return Ok(Runs::Workgroups(workgroups)),
        Invocations::Indirect { storage, at } => {
            let bytes = kept.storage(storage).ok_or(DispatchRefused::Storage)?;
            if !storage::fits(bytes, at, DISPATCH_ARGUMENTS) {
                return Err(DispatchRefused::Arguments { at, bytes });
            }
            return Ok(Runs::Indirect { storage, at });
        }
        Invocations::PerElement(storage) => storage,
    };
    let array = bindings
        .iter()
        .filter(|bind| bind.to == Resource::Storage(storage))
        .find_map(|bind| compute.arrays.iter().find(|array| array.group == bind.group && array.binding == bind.binding))
        .ok_or(DispatchRefused::Elements)?;
    let bytes = kept.storage(storage).ok_or(DispatchRefused::Elements)?;

    Ok(Runs::Workgroups([array.elements(bytes).div_ceil(compute.workgroup[0]), 1, 1]))
}

/// Checks that `join`, using storage as `accessed` says, has no frames read each other round, with what is read already,
/// and that what it writes no other dispatch of its pass uses, nor reads what one writes, and no dispatch of a frame
/// it is in no order with writes.
fn check_order(join: &Dispatch, accessed: &[(StorageId, Access)], kept: &impl Finds) -> Result<(), DispatchRefused> {
    let mut reads = kept.reads();
    if let Frame::Texture(reader) = join.frame {
        for bind in &join.bindings {
            let Resource::Texture(texture) = bind.to else {
                continue;
            };
            let Some(read) = kept.drawn_by(texture) else {
                continue;
            };
            if frame::reaches(&reads, read, reader.id) {
                return Err(DispatchRefused::Round);
            }
            reads.push((reader.id, read));
        }
    }
    let uses: Vec<(Use, Use)> = accessed
        .iter()
        .flat_map(|(storage, access)| {
            let mine = Use { frame: join.frame, pass: join.pass, access: *access };
            kept.uses(*storage).into_iter().map(move |theirs| (mine, theirs))
        })
        .collect();
    for (mine, theirs) in &uses {
        let Some((reader, read)) = mine.reads_from(*theirs) else {
            continue;
        };
        if frame::reaches(&reads, read, reader) {
            return Err(DispatchRefused::Round);
        }
        reads.push((reader, read));
    }
    for (mine, theirs) in uses {
        let writes = mine.access == Access::Write;
        let either = writes || theirs.access == Access::Write;
        let both = writes && theirs.access == Access::Write;
        let same_pass = mine.same_pass(theirs);
        let in_one_pass = same_pass && either;
        let unordered = both && !same_pass && !ordered(mine.frame, theirs.frame, &reads);
        if in_one_pass || unordered {
            return Err(DispatchRefused::Written);
        }
    }

    Ok(())
}

/// Each dispatch of `writers`, each a dispatch, its frame, and a storage it writes, that writes a storage another
/// writes in a frame it is in no order with, as `reads` holds the frames' reads: what a released placement leaves of
/// two writers it ordered when they were made, which are not run until their frames are ordered again.
pub(crate) fn unordered(writers: &[(u64, Frame, u64)], reads: &[(u64, u64)]) -> Vec<u64> {
    let mut held: Vec<u64> = writers
        .iter()
        .filter(|(id, frame, storage)| {
            writers.iter().any(|(other, theirs, written)| {
                let rival = other != id && written == storage;
                rival && !ordered(*frame, *theirs, reads)
            })
        })
        .map(|(id, _, _)| *id)
        .collect();
    held.sort_unstable();
    held.dedup();

    held
}

/// Whether frames `one` and `other` run in an order: one frame, a frame that ends on a texture and a window's main
/// frame, which runs after every such frame, or two that end on textures one of which reads the other, through
/// frames between or at once, as `reads` holds.
fn ordered(one: Frame, other: Frame, reads: &[(u64, u64)]) -> bool {
    match (one, other) {
        (Frame::Texture(one), Frame::Texture(other)) => {
            frame::reaches(reads, one.id, other.id) || frame::reaches(reads, other.id, one.id)
        }
        (Frame::Main(one), Frame::Main(other)) => one == other,
        (Frame::Main(_), Frame::Texture(_)) | (Frame::Texture(_), Frame::Main(_)) => true,
    }
}
