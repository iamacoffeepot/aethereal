//! Placements: one thing drawn, a mesh with a program for each of a count of instances, with what the program binds,
//! asked for by any actor and checked as it is made, so drawing it cannot fail (ADR-0023).

#[cfg(test)]
mod tests;

use crate::instances::InstancesId;
use crate::mesh::{MeshId, VertexLayout};
use crate::program::{Binding, BindingKind, PerInstance, ProgramId};
use crate::texture::TextureId;
use crate::uniforms::UniformsId;
use aethereal_data::{Kind, KindName, Plain};

/// Asks the renderer to draw `mesh` with `program`, for each of `instances`, binding what `bindings` names at each
/// place the program binds, at every frame until it is released. Answered with [`Placed`].
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct Place {
    pub program: ProgramId,
    pub mesh: MeshId,
    pub instances: Drawn,
    pub bindings: Vec<Bind>,
}

impl Kind for Place {
    const NAME: KindName = KindName::new("aether.render.place");
}

/// How many times a placement is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Drawn {
    /// This many times, for a program that reads nothing per instance.
    Count(u32),
    /// Once for each record these instances hold at each frame, for a program that reads records of their layout.
    Records(InstancesId),
}

impl Kind for Drawn {
    const NAME: KindName = KindName::new("aether.render.drawn");
}

/// What a placement binds at one place its program binds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct Bind {
    pub group: u32,
    pub binding: u32,
    pub to: Resource,
}

impl Kind for Bind {
    const NAME: KindName = KindName::new("aether.render.bind");
}

/// What is bound: uniforms to a uniform block, a texture to a texture, or a way of sampling to a sampler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Resource {
    Uniforms(UniformsId),
    Texture(TextureId),
    Sampler(Sampling),
}

impl Kind for Resource {
    const NAME: KindName = KindName::new("aether.render.resource");
}

/// How a sampler reads a texture. The renderer keeps one sampler for each way, so none is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
pub struct Sampling {
    pub filter: Filter,
    pub wrap: Wrap,
}

impl Kind for Sampling {
    const NAME: KindName = KindName::new("aether.render.sampling");
}

/// How a sampler reads between texels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
pub enum Filter {
    /// It blends the nearest texels.
    Linear,
    /// It takes the nearest texel.
    Nearest,
}

impl Kind for Filter {
    const NAME: KindName = KindName::new("aether.render.filter");
}

/// How a sampler reads past a texture's edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
pub enum Wrap {
    /// It reads the edge.
    Clamp,
    /// It reads the texture again.
    Repeat,
}

impl Kind for Wrap {
    const NAME: KindName = KindName::new("aether.render.wrap");
}

/// A placement the renderer made, which only its maker may release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct PlacementId {
    pub(crate) id: u64,
}

impl Kind for PlacementId {
    const NAME: KindName = KindName::new("aether.render.placement_id");
}

/// What the renderer answers [`Place`] with.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub enum Placed {
    Made(PlacementId),
    Refused(PlacementRefused),
}

impl Kind for Placed {
    const NAME: KindName = KindName::new("aether.render.placed");
}

/// Why a placement was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum PlacementRefused {
    /// No program is kept under the id: never made, or released.
    Program,
    /// No mesh is kept under the id.
    Mesh,
    /// No instances are kept under the id.
    Instances,
    /// The mesh's layout is not the program's vertex layout.
    Layout,
    /// What is drawn is not what the program reads per instance: records for a program that reads none, a count for
    /// one that reads records, or records of another layout.
    Drawn,
    /// The program binds a place nothing is bound to.
    Unbound { group: u32, binding: u32 },
    /// Something is bound to a place the program does not bind.
    Unlisted { group: u32, binding: u32 },
    /// Two are bound to one place.
    Repeated { group: u32, binding: u32 },
    /// What is bound names nothing kept: uniforms or a texture never made, or released.
    Missing { group: u32, binding: u32 },
    /// What is bound is not of the kind the program binds there, or uniforms are smaller than its block.
    Mismatch { group: u32, binding: u32 },
}

impl Kind for PlacementRefused {
    const NAME: KindName = KindName::new("aether.render.placement_refused");
}

/// What a placement is checked against of the program it draws with.
pub(crate) struct Shape<'a> {
    pub layout: &'a VertexLayout,
    pub instances: &'a PerInstance,
    pub bindings: &'a [Binding],
}

/// What the renderer keeps, as a placement's check looks it up: each lookup misses when nothing of its sort is kept
/// under the id.
pub(crate) trait Finds {
    fn program(&self, id: ProgramId) -> Option<Shape<'_>>;
    fn mesh(&self, id: MeshId) -> Option<&VertexLayout>;
    fn instances(&self, id: InstancesId) -> Option<&VertexLayout>;
    fn uniforms(&self, id: UniformsId) -> Option<u32>;
    fn has_texture(&self, id: TextureId) -> bool;
}

/// Checks a placement before it is made: that what it names is kept, that the mesh and the instances are laid out as
/// the program reads them, and that it binds each place the program binds, once, with a resource of the kind there.
pub(crate) fn check(place: &Place, kept: &impl Finds) -> Result<(), PlacementRefused> {
    let program = kept.program(place.program).ok_or(PlacementRefused::Program)?;
    let mesh = kept.mesh(place.mesh).ok_or(PlacementRefused::Mesh)?;
    if mesh != program.layout {
        return Err(PlacementRefused::Layout);
    }
    match (place.instances, program.instances) {
        (Drawn::Count(_), PerInstance::None) => {}
        (Drawn::Records(instances), PerInstance::Records(read)) => {
            let layout = kept.instances(instances).ok_or(PlacementRefused::Instances)?;
            if layout != read {
                return Err(PlacementRefused::Drawn);
            }
        }
        (Drawn::Count(_), PerInstance::Records(_)) | (Drawn::Records(_), PerInstance::None) => {
            return Err(PlacementRefused::Drawn);
        }
    }

    for (at, bind) in place.bindings.iter().enumerate() {
        let (group, binding) = (bind.group, bind.binding);
        let repeated = place.bindings[..at].iter().any(|before| before.group == group && before.binding == binding);
        if repeated {
            return Err(PlacementRefused::Repeated { group, binding });
        }
        let listed = program.bindings.iter().find(|listed| listed.group == group && listed.binding == binding);
        let Some(listed) = listed else {
            return Err(PlacementRefused::Unlisted { group, binding });
        };
        let fits = match (bind.to, listed.kind) {
            (Resource::Uniforms(uniforms), BindingKind::Uniform { bytes }) => {
                let size = kept.uniforms(uniforms).ok_or(PlacementRefused::Missing { group, binding })?;
                size >= bytes
            }
            (Resource::Texture(texture), BindingKind::Texture) => {
                if !kept.has_texture(texture) {
                    return Err(PlacementRefused::Missing { group, binding });
                }
                true
            }
            (Resource::Sampler(_), BindingKind::Sampler) => true,
            _ => false,
        };
        if !fits {
            return Err(PlacementRefused::Mismatch { group, binding });
        }
    }
    let unbound = program
        .bindings
        .iter()
        .find(|listed| !place.bindings.iter().any(|bind| bind.group == listed.group && bind.binding == listed.binding));
    if let Some(listed) = unbound {
        return Err(PlacementRefused::Unbound { group: listed.group, binding: listed.binding });
    }

    Ok(())
}
