//! Placements: one thing drawn, a mesh for each of a count of instances, joining passes of frames each with a program
//! and what the program binds, asked for by any actor and checked as it is made, so drawing it cannot fail (ADR-0023,
//! ADR-0025).

#[cfg(test)]
mod tests;

use crate::frame::{self, Frame};
use crate::instances::InstancesId;
use crate::mesh::{MeshId, VertexLayout};
use crate::pass::{Pass, PassName};
use crate::program::{Binding, BindingKind, PerInstance, ProgramId};
use crate::shape::Shape;
use crate::storage::StorageId;
use crate::texture::TextureId;
use crate::uniforms::UniformsId;
use aethereal_data::{Kind, KindName, Plain};

/// Asks the renderer to draw `mesh` for each of `instances` in every pass `joins` names, once in each, at every frame
/// until it is released. Answered with [`Placed`].
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct Place {
    pub mesh: MeshId,
    pub instances: Drawn,
    pub joins: Vec<Join>,
}

impl Kind for Place {
    const NAME: KindName = KindName::new("aether.render.place");
}

/// One pass a placement draws in: the pass `pass` of `frame`, drawn with `program`, binding what `bindings` names at
/// each place the program binds.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct Join {
    pub frame: Frame,
    pub pass: PassName,
    pub program: ProgramId,
    pub bindings: Vec<Bind>,
}

impl Kind for Join {
    const NAME: KindName = KindName::new("aether.render.join");
}

impl Join {
    /// The pass `P` of `frame`, drawn with `program`, binding `bindings`: the pass named by its type.
    #[must_use]
    pub const fn to<P: Pass>(frame: Frame, program: ProgramId, bindings: Vec<Bind>) -> Self {
        Self { frame, pass: PassName::of::<P>(), program, bindings }
    }
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

/// What is bound: uniforms to a uniform block, storage to a storage buffer, a texture to a texture, or a way of
/// sampling to a sampler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Resource {
    Uniforms(UniformsId),
    Storage(StorageId),
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
    /// No mesh is kept under the id: never made, or released.
    Mesh,
    /// No instances are kept under the id.
    Instances,
    /// It joins no pass, and would draw nothing.
    Unjoined,
    /// Its join at `join`, counted from zero, was refused.
    Join { join: u32, why: JoinRefused },
}

impl Kind for PlacementRefused {
    const NAME: KindName = KindName::new("aether.render.placement_refused");
}

/// Why one join of a placement was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum JoinRefused {
    /// No frame is kept as the one named.
    Frame,
    /// The frame has no pass of the name.
    Pass,
    /// An earlier join of the placement joins the same pass of the same frame.
    Twice,
    /// It binds the texture of the frame it joins, or of a frame that reads that frame, through frames between or at
    /// once: frames would read each other round, each what the other drew a `Render` before.
    Round,
    /// No program is kept under the id: never made, or released.
    Program,
    /// The program is made against another shape than the pass draws into.
    Shape,
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
    /// What is bound names nothing kept: uniforms, storage, or a texture never made, or released.
    Missing { group: u32, binding: u32 },
    /// What is bound is not of the kind the program binds there, or uniforms or storage are smaller than the program
    /// binds there.
    Mismatch { group: u32, binding: u32 },
}

impl Kind for JoinRefused {
    const NAME: KindName = KindName::new("aether.render.join_refused");
}

/// What a join is checked against of the program it draws with: what it reads, what it binds, and the shape it was
/// made against.
pub(crate) struct Reads<'a> {
    pub layout: &'a VertexLayout,
    pub instances: &'a PerInstance,
    pub bindings: &'a [Binding],
    pub shape: Shape,
}

/// What the renderer keeps, as a placement's check looks it up: each lookup misses when nothing of its sort is kept
/// under the id.
pub(crate) trait Finds {
    /// The shape of the pass `pass` of `frame` as it is now: refused when there is no such frame, or it has no such
    /// pass.
    fn pass(&self, frame: Frame, pass: PassName) -> Result<Shape, JoinRefused>;
    fn program(&self, id: ProgramId) -> Option<Reads<'_>>;
    /// The frame that ends on the texture `texture`, by its id: none for a texture made of pixels, or none kept.
    fn drawn_by(&self, texture: TextureId) -> Option<u64>;
    /// Each pair of a frame that ends on a texture and a frame whose texture it reads, by their ids: through what
    /// the placements that join it bind, and what its effects bind.
    fn reads(&self) -> Vec<(u64, u64)>;
    fn mesh(&self, id: MeshId) -> Option<&VertexLayout>;
    fn instances(&self, id: InstancesId) -> Option<&VertexLayout>;
    fn uniforms(&self, id: UniformsId) -> Option<u32>;
    fn storage(&self, id: StorageId) -> Option<u32>;
    fn has_texture(&self, id: TextureId) -> bool;
}

/// Checks a placement before it is made: that its mesh and instances are kept, that it joins some pass, and that each
/// join is one [`check_join`] lets through.
pub(crate) fn check(place: &Place, kept: &impl Finds) -> Result<(), PlacementRefused> {
    let mesh = kept.mesh(place.mesh).ok_or(PlacementRefused::Mesh)?;
    let instancing = match place.instances {
        Drawn::Count(_) => Instancing::Count,
        Drawn::Records(instances) => Instancing::Records(kept.instances(instances).ok_or(PlacementRefused::Instances)?),
    };
    if place.joins.is_empty() {
        return Err(PlacementRefused::Unjoined);
    }
    for (join, at) in place.joins.iter().zip(0..) {
        let twice =
            place.joins[..at as usize].iter().any(|before| before.frame == join.frame && before.pass == join.pass);
        let checked = if twice {
            Err(JoinRefused::Twice)
        } else {
            check_join(join, mesh, instancing, kept)
        };
        checked.map_err(|why| PlacementRefused::Join { join: at, why })?;
    }
    check_round(place, kept)
}

/// Checks that no join of `place` has frames read each other round: one that joins a frame ending on a texture and
/// binds the texture of a frame that reads it, with what is read already and what its earlier joins read.
fn check_round(place: &Place, kept: &impl Finds) -> Result<(), PlacementRefused> {
    let mut reads = kept.reads();
    for (join, at) in place.joins.iter().zip(0..) {
        let Frame::Texture(reader) = join.frame else {
            continue;
        };
        for bind in &join.bindings {
            let Resource::Texture(texture) = bind.to else {
                continue;
            };
            let Some(read) = kept.drawn_by(texture) else {
                continue;
            };
            if frame::reaches(&reads, read, reader.id) {
                return Err(PlacementRefused::Join { join: at, why: JoinRefused::Round });
            }
            reads.push((reader.id, read));
        }
    }

    Ok(())
}

/// How a placement's instances are read: a count, or records laid out so.
#[derive(Clone, Copy)]
enum Instancing<'a> {
    Count,
    Records(&'a VertexLayout),
}

/// Checks one join of a placement of a mesh laid out as `mesh`, its instances read as `instancing` says: that the frame
/// has the pass, that the program is kept and made against the pass's shape, that the mesh and the records are laid
/// out as the program reads them, and that it binds each place the program binds, once, with a resource of the kind
/// there.
fn check_join(
    join: &Join,
    mesh: &VertexLayout,
    instancing: Instancing<'_>,
    kept: &impl Finds,
) -> Result<(), JoinRefused> {
    let pass = kept.pass(join.frame, join.pass)?;
    let program = kept.program(join.program).ok_or(JoinRefused::Program)?;
    if program.shape != pass {
        return Err(JoinRefused::Shape);
    }
    if mesh != program.layout {
        return Err(JoinRefused::Layout);
    }
    let drawn = match (instancing, program.instances) {
        (Instancing::Count, PerInstance::None) => true,
        (Instancing::Records(layout), PerInstance::Records(read)) => layout == read,
        (Instancing::Count, PerInstance::Records(_)) | (Instancing::Records(_), PerInstance::None) => false,
    };
    if !drawn {
        return Err(JoinRefused::Drawn);
    }

    check_bindings(&join.bindings, program.bindings, kept)
}

/// Checks that `bindings` bind each place `listed` lists, once, and nothing else, each with a resource kept of the kind
/// listed there, and uniforms or storage of at least its bytes.
pub(crate) fn check_bindings(bindings: &[Bind], listed: &[Binding], kept: &impl Finds) -> Result<(), JoinRefused> {
    for (at, bind) in bindings.iter().enumerate() {
        let (group, binding) = (bind.group, bind.binding);
        let repeated = bindings[..at].iter().any(|before| before.group == group && before.binding == binding);
        if repeated {
            return Err(JoinRefused::Repeated { group, binding });
        }
        let found = listed.iter().find(|listed| listed.group == group && listed.binding == binding);
        let Some(found) = found else {
            return Err(JoinRefused::Unlisted { group, binding });
        };
        let fits = match (bind.to, found.kind) {
            (Resource::Uniforms(uniforms), BindingKind::Uniform { bytes }) => {
                let size = kept.uniforms(uniforms).ok_or(JoinRefused::Missing { group, binding })?;
                size >= bytes
            }
            (Resource::Storage(storage), BindingKind::Storage { bytes }) => {
                let size = kept.storage(storage).ok_or(JoinRefused::Missing { group, binding })?;
                size >= bytes
            }
            (Resource::Texture(texture), BindingKind::Texture) => {
                if !kept.has_texture(texture) {
                    return Err(JoinRefused::Missing { group, binding });
                }
                true
            }
            (Resource::Sampler(_), BindingKind::Sampler) => true,
            _ => false,
        };
        if !fits {
            return Err(JoinRefused::Mismatch { group, binding });
        }
    }
    let unbound = listed
        .iter()
        .find(|listed| !bindings.iter().any(|bind| bind.group == listed.group && bind.binding == listed.binding));
    if let Some(listed) = unbound {
        return Err(JoinRefused::Unbound { group: listed.group, binding: listed.binding });
    }

    Ok(())
}
