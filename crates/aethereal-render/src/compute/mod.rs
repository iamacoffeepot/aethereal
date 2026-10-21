//! Compute programs: WGSL a compute pipeline is made from, with what it binds, asked for by any actor and checked as
//! they are made, as programs are, so a dispatch with one cannot fail (ADR-0026).

#[cfg(test)]
mod tests;

use crate::program::{Binding, ProgramRefused, Stage, check_bindings, check_uses, entry, listed, parsed};
use aethereal_data::{Kind, KindName, Plain};
use naga::valid::ModuleInfo;
use naga::{AddressSpace, ArraySize, Handle, Module, Type, TypeInner};

/// Asks the renderer for a compute program: the compute stage `entry` of the WGSL `wgsl`, binding what `bindings`
/// lists, storage to read or to write. Answered with [`Computed`].
///
/// Its workgroup size is read from the stage's `@workgroup_size`, which is constant, as a module that declares a
/// constant a pipeline overrides is refused.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct CreateCompute {
    pub wgsl: String,
    pub entry: String,
    pub bindings: Vec<Binding>,
}

impl Kind for CreateCompute {
    const NAME: KindName = KindName::new("aether.render.create_compute");
}

/// A compute program the renderer made, which any actor handed it may dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct ComputeId {
    pub(crate) id: u64,
}

impl Kind for ComputeId {
    const NAME: KindName = KindName::new("aether.render.compute_id");
}

/// What the renderer answers [`CreateCompute`] with.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub enum Computed {
    Made(ComputeId),
    Refused(ProgramRefused),
}

impl Kind for Computed {
    const NAME: KindName = KindName::new("aether.render.computed");
}

/// What a compute program's check reads of its shader: the size of its workgroups, which a dispatch counts the
/// workgroups it runs by, and each storage buffer it uses whose type ends in an array whose length is its buffer's,
/// which a dispatch of one invocation for each element counts its elements by (ADR-0026).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub workgroup: [u32; 3],
    pub arrays: Vec<Array>,
}

/// A storage buffer whose type ends in an array whose length is its buffer's (`array<T>`): where it is bound, the
/// bytes before the array, and the stride of the array's elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Array {
    pub group: u32,
    pub binding: u32,
    pub fixed: u32,
    pub stride: u32,
}

impl Array {
    /// How many elements the array holds in storage of `bytes`, which is at least what the binding reads: those that
    /// fit past its fixed part.
    #[must_use]
    pub const fn elements(self, bytes: u32) -> u32 {
        bytes.saturating_sub(self.fixed) / self.stride
    }
}

/// Checks a compute program before it is made: that its WGSL is a valid module whose entry point is a compute stage,
/// binding only what `bindings` lists, each as it is listed, within `limits`, and running workgroups no larger than
/// `limits` lets one be, in invocations and in workgroup memory. Answers the size of its workgroups, and the storage
/// it uses that ends in an array whose length is its buffer's.
///
/// # Errors
///
/// Why it would not be made as asked.
pub fn check(create: &CreateCompute, limits: &wgpu::Limits) -> Result<Checked, ProgramRefused> {
    check_bindings(&create.bindings, 0, limits)?;

    let (module, info) = parsed(&create.wgsl)?;
    let at = entry(&module, Stage::Compute, &create.entry)?;
    check_uses(&module, &info, at, |group, binding| listed(&create.bindings, group, binding))?;
    let workgroup = module.entry_points[at].workgroup_size;
    check_workgroup(workgroup, limits)?;
    check_workgroup_memory(&module, &info, at, limits)?;
    let uses = info.get_entry_point(at);
    let arrays = module
        .global_variables
        .iter()
        .filter(|(handle, global)| matches!(global.space, AddressSpace::Storage { .. }) && !uses[*handle].is_empty())
        .filter_map(|(_, global)| {
            let place = global.binding.as_ref()?;
            let (fixed, stride) = ends_in_array(&module, global.ty)?;
            Some(Array { group: place.group, binding: place.binding, fixed, stride })
        })
        .collect();

    Ok(Checked { workgroup, arrays })
}

/// Where the array whose length is its buffer's that `ty` ends in begins, and its stride: the type itself, or the
/// last member of a struct. None when it ends in no such array.
fn ends_in_array(module: &Module, ty: Handle<Type>) -> Option<(u32, u32)> {
    let runtime = |ty: Handle<Type>| match module.types[ty].inner {
        TypeInner::Array { size: ArraySize::Dynamic, stride, .. } => Some(stride),
        _ => None,
    };
    match &module.types[ty].inner {
        TypeInner::Struct { members, .. } => {
            let last = members.last()?;
            Some((last.offset, runtime(last.ty)?))
        }
        _ => Some((0, runtime(ty)?)),
    }
}

/// Checks that a workgroup of `size` invocations along each dimension is one the GPU runs: no more along each than it
/// runs along that one, and no more in all than it runs in one workgroup.
fn check_workgroup(size: [u32; 3], limits: &wgpu::Limits) -> Result<(), ProgramRefused> {
    let [x, y, z] = size;
    let along = x <= limits.max_compute_workgroup_size_x
        && y <= limits.max_compute_workgroup_size_y
        && z <= limits.max_compute_workgroup_size_z;
    let invocations = u64::from(x) * u64::from(y) * u64::from(z);
    let in_all = invocations <= u64::from(limits.max_compute_invocations_per_workgroup);
    let runs = along && in_all;
    if !runs {
        return Err(ProgramRefused::Workgroup { size });
    }

    Ok(())
}

/// Checks that the workgroup memory the entry point `at` uses is no more than the GPU gives a workgroup, each
/// variable counted as WebGPU counts it, its size rounded up to sixteen bytes.
fn check_workgroup_memory(
    module: &Module,
    info: &ModuleInfo,
    at: usize,
    limits: &wgpu::Limits,
) -> Result<(), ProgramRefused> {
    let uses = info.get_entry_point(at);
    let bytes: u64 = module
        .global_variables
        .iter()
        .filter(|(handle, global)| global.space == AddressSpace::WorkGroup && !uses[*handle].is_empty())
        .map(|(_, global)| u64::from(module.types[global.ty].inner.size(module.to_ctx())).next_multiple_of(16))
        .sum();
    if bytes > u64::from(limits.max_compute_workgroup_storage_size) {
        return Err(ProgramRefused::WorkgroupMemory { bytes });
    }

    Ok(())
}
