//! Programs: WGSL a pipeline is made from, with the vertex buffer it reads, what it binds, and its fixed state, asked
//! for by any actor and checked as they are made, so a draw with one cannot fail (ADR-0021).

#[cfg(test)]
mod tests;

use crate::mesh::{self, MeshRefused, VertexFormat, VertexLayout};
use crate::shape::{ColourFormat, DepthFormat, Shape};
use aethereal_actor::BlobRef;
use aethereal_data::{Bound, Kind, KindName, Plain};
use naga::front::wgsl;
use naga::valid::{Capabilities, ModuleInfo, ValidationFlags, Validator};
use naga::{
    AddressSpace, Binding as Bound, ImageClass, ImageDimension, Module, ScalarKind, ShaderStage, StorageAccess,
    TypeInner,
};

/// Asks the renderer for a program: the WGSL the blob `wgsl` holds as UTF-8, drawn from its vertex stage `vertex` and
/// its fragment stage `fragment`, reading vertices laid out as `layout` says and what `instances` says once per
/// instance, binding what `bindings` lists, with the fixed state `state`, drawing into passes of the shape `shape`
/// (ADR-0025). Answered with [`Programmed`].
///
/// The WGSL travels as a blob, as a mesh's vertices do, so a shader longer than the engine reads of one piece of mail
/// can still be asked for.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
pub struct CreateProgram {
    pub wgsl: BlobRef,
    pub vertex: String,
    pub fragment: String,
    pub layout: VertexLayout,
    pub instances: PerInstance,
    pub bindings: Vec<Binding>,
    pub state: Fixed,
    pub shape: Shape,
}

impl Kind for CreateProgram {
    const NAME: KindName = KindName::new("aether.render.create_program");
}

/// What a program's vertex stage reads once per instance (ADR-0023).
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub enum PerInstance {
    /// Nothing: it is drawn a count of times, reading only its vertices.
    None,
    /// A record laid out so, from a second vertex buffer stepped once per instance, at locations the vertex layout
    /// does not read.
    Records(VertexLayout),
}

impl Kind for PerInstance {
    const NAME: KindName = KindName::new("aether.render.per_instance");
}

/// One thing a program binds: where, by its group and its binding in the group, and what.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct Binding {
    pub group: u32,
    pub binding: u32,
    pub kind: BindingKind,
}

impl Kind for Binding {
    const NAME: KindName = KindName::new("aether.render.binding");
}

/// What a program binds at one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum BindingKind {
    /// A uniform block of `bytes`, at least what the shader reads of it.
    Uniform { bytes: u32 },
    /// A storage buffer the shader reads, or writes, as `access` says, of at least `bytes`: what the shader reads of
    /// it with any array whose length is the buffer's holding one element. Storage bound there is at least as large,
    /// and such an array holds as many elements as fit in the rest (ADR-0023). A draw program or an effect binds it
    /// only to read, from either stage; a compute program binds it either way (ADR-0026).
    Storage { bytes: u32, access: Access },
    /// A two-dimensional texture of floats, as every [`TextureFormat`](crate::texture::TextureFormat) is read.
    Texture,
    /// A sampler that filters.
    Sampler,
}

impl Kind for BindingKind {
    const NAME: KindName = KindName::new("aether.render.binding_kind");
}

/// How a shader binds a storage buffer: what a program says it does with it is what its shader declares (ADR-0026).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Access {
    /// It only reads it, `var<storage, read>`.
    Read,
    /// It reads and writes it, `var<storage, read_write>`.
    Write,
}

impl Kind for Access {
    const NAME: KindName = KindName::new("aether.render.access");
}

/// What a program fixes as it is made: how it blends, how it tests depth, and which faces it culls. It draws
/// triangles, counter-clockwise in front.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct Fixed {
    pub blend: Blend,
    pub depth: Depth,
    pub cull: Cull,
}

impl Kind for Fixed {
    const NAME: KindName = KindName::new("aether.render.fixed");
}

/// How a program's colour is laid over what is drawn already.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Blend {
    /// It replaces it.
    Opaque,
    /// It is laid over it by its alpha.
    Alpha,
    /// It is added to it.
    Additive,
    /// It multiplies it, channel by channel, darkening it as a tint or a shadow does; the alpha drawn already is kept.
    Multiply,
}

impl Kind for Blend {
    const NAME: KindName = KindName::new("aether.render.blend");
}

/// How a program tests and writes depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Depth {
    /// It neither tests nor writes it.
    Off,
    /// It draws only what is nearer than what is drawn, and writes nothing.
    Test,
    /// It draws only what is nearer, and writes its own depth.
    TestWrite,
}

impl Kind for Depth {
    const NAME: KindName = KindName::new("aether.render.depth");
}

/// Which faces of its triangles a program does not draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Cull {
    None,
    Back,
    Front,
}

impl Kind for Cull {
    const NAME: KindName = KindName::new("aether.render.cull");
}

/// A program the renderer made, which any actor handed it may draw with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct ProgramId {
    pub(crate) id: u64,
}

impl Kind for ProgramId {
    const NAME: KindName = KindName::new("aether.render.program_id");
}

/// What the renderer answers [`CreateProgram`] with.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub enum Programmed {
    Made(ProgramId),
    Refused(ProgramRefused),
}

impl Kind for Programmed {
    const NAME: KindName = KindName::new("aether.render.programmed");
}

/// One of a program's two stages, or a compute program's one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Stage {
    Vertex,
    Fragment,
    Compute,
}

impl Kind for Stage {
    const NAME: KindName = KindName::new("aether.render.stage");
}

/// Why a program was not made.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub enum ProgramRefused {
    /// The WGSL's bytes are not UTF-8: the first `valid` of them are.
    Utf8 { valid: u64 },
    /// The WGSL does not parse; `message` says where and why.
    Parse { message: String },
    /// The WGSL parses, and is not a valid module; `message` says where and why.
    Invalid { message: String },
    /// The module declares a constant that a pipeline overrides, which a program does not set.
    Overrides,
    /// The vertex layout is one no mesh could have.
    Layout { why: MeshRefused },
    /// The instance layout is one no instances could have.
    InstanceLayout { why: MeshRefused },
    /// The vertex and instance layouts both read `location`.
    Shared { location: u32 },
    /// The module has no entry point `name` of the stage.
    Entry { stage: Stage, name: String },
    /// The vertex stage reads a location neither layout gives an attribute at.
    Input { location: u32 },
    /// The vertex stage reads an attribute as another kind of number than its format is.
    InputFormat { location: u32 },
    /// The fragment stage reads a location the vertex stage does not write, or writes as another type or
    /// interpolation.
    Stages { location: u32 },
    /// The fragment stage does not write the colour drawn at location 0: a `vec4<f32>`, or for a shape of whole
    /// numbers a `u32` or a vector of them.
    Output,
    /// It blends into a shape of whole numbers, which nothing blends.
    Blend,
    /// It tests or writes depth, and its shape holds none.
    Depthless,
    /// An effect's shape holds depth: an effect draws over a whole target, and tests none.
    EffectDepth,
    /// An effect lists a binding in group 0, where what it reads is bound.
    Reserved { binding: u32 },
    /// The shader uses a binding `bindings` does not list.
    Unbound { group: u32, binding: u32 },
    /// The shader uses a binding as another kind than `bindings` lists, or reads more of a uniform block or a storage
    /// buffer than its bytes, or declares a storage buffer to be written that is listed to be read, or to be read that
    /// is listed to be written, or binds what a program cannot: a texture that is not two-dimensional floats.
    Mismatch { group: u32, binding: u32 },
    /// A draw program or an effect lists storage to write, which only a compute program writes (ADR-0026).
    Writes { group: u32, binding: u32 },
    /// `bindings` lists two at one place.
    Repeated { group: u32, binding: u32 },
    /// A binding's group is past the last the GPU has.
    Group { group: u32 },
    /// A uniform block or a storage buffer is empty, or larger than the GPU binds.
    Size { group: u32, binding: u32 },
    /// `bindings` lists more of one kind than the GPU binds in a stage.
    Many { kind: BindingKind },
    /// A compute program's workgroup, of `size` invocations along each dimension, is more than the GPU runs: more
    /// along one dimension, or more in all.
    Workgroup { size: [u32; 3] },
    /// A compute program's workgroup memory, `bytes` as WebGPU counts it, is more than the GPU gives a workgroup.
    WorkgroupMemory { bytes: u64 },
}

impl Kind for ProgramRefused {
    const NAME: KindName = KindName::new("aether.render.program_refused");
}

/// The WGSL a request's blob holds, `bytes`, as text: refused when they are not UTF-8.
///
/// # Errors
///
/// [`ProgramRefused::Utf8`], with how many of the bytes are.
pub fn source(bytes: &[u8]) -> Result<&str, ProgramRefused> {
    str::from_utf8(bytes)
        .map_err(|error| ProgramRefused::Utf8 { valid: u64::try_from(error.valid_up_to()).unwrap_or(u64::MAX) })
}

/// Checks a program before it is made, with `wgsl` the WGSL its blob holds: that the WGSL is a valid module, whose
/// entry points are a vertex and a fragment stage, that read what its layouts give, pass between them what each reads,
/// write the colour drawn, and bind only what `bindings` lists, storage only to read, within `limits`: what WebGPU
/// would refuse as the pipeline is made.
///
/// # Errors
///
/// Why it would not be made as asked.
pub fn check(create: &CreateProgram, wgsl: &str, limits: &wgpu::Limits) -> Result<(), ProgramRefused> {
    mesh::check_layout(&create.layout).map_err(|why| ProgramRefused::Layout { why })?;
    if let PerInstance::Records(instances) = &create.instances {
        mesh::check_layout(instances).map_err(|why| ProgramRefused::InstanceLayout { why })?;
        let shared = instances.attributes.iter().find(|attribute| create.layout.reads(attribute.location));
        if let Some(attribute) = shared {
            return Err(ProgramRefused::Shared { location: attribute.location });
        }
    }
    check_bindings(&create.bindings, 0, limits)?;
    check_reads_only(&create.bindings)?;
    check_fixed(create.state.blend, create.shape)?;
    let tests_depth = create.state.depth != Depth::Off;
    let holds_depth = create.shape.depth != DepthFormat::None;
    if tests_depth && !holds_depth {
        return Err(ProgramRefused::Depthless);
    }

    let (module, info) = parsed(wgsl)?;
    let vertex = entry(&module, Stage::Vertex, &create.vertex)?;
    let fragment = entry(&module, Stage::Fragment, &create.fragment)?;
    check_inputs(&module, vertex, &create.layout, &create.instances)?;
    check_stages((&module, vertex), (&module, fragment))?;
    check_output(&module, fragment, create.shape.colour)?;
    for at in [vertex, fragment] {
        check_uses(&module, &info, at, |group, binding| listed(&create.bindings, group, binding))?;
    }

    Ok(())
}

/// The module `wgsl` is, validated, with what its validation found: refused when it does not parse, is not valid, or
/// declares a constant a pipeline overrides.
pub(crate) fn parsed(wgsl: &str) -> Result<(Module, ModuleInfo), ProgramRefused> {
    let module =
        wgsl::parse_str(wgsl).map_err(|error| ProgramRefused::Parse { message: error.emit_to_string(wgsl) })?;
    let info = Validator::new(ValidationFlags::all(), Capabilities::empty())
        .validate(&module)
        .map_err(|error| ProgramRefused::Invalid { message: error.emit_to_string(wgsl) })?;
    if !module.overrides.is_empty() {
        return Err(ProgramRefused::Overrides);
    }

    Ok((module, info))
}

/// Checks that a program blending by `blend` may draw into `shape`: nothing blends into whole numbers.
pub(crate) fn check_fixed(blend: Blend, shape: Shape) -> Result<(), ProgramRefused> {
    let blends = blend != Blend::Opaque;
    if blends && shape.colour.is_whole() {
        return Err(ProgramRefused::Blend);
    }

    Ok(())
}

/// The kind `bindings` lists at `group`, `binding`: none when it lists nothing there.
pub(crate) fn listed(bindings: &[Binding], group: u32, binding: u32) -> Option<BindingKind> {
    bindings.iter().find(|listed| listed.group == group && listed.binding == binding).map(|listed| listed.kind)
}

/// Checks that each binding is at a place of its own, in a group the GPU has, a uniform block or a storage buffer of a
/// size it binds, and that there are no more of each kind than a stage binds, `textures` more besides counted with
/// the textures.
pub(crate) fn check_bindings(bindings: &[Binding], textures: u32, limits: &wgpu::Limits) -> Result<(), ProgramRefused> {
    for (at, listed) in bindings.iter().enumerate() {
        let (group, binding) = (listed.group, listed.binding);
        if group >= limits.max_bind_groups {
            return Err(ProgramRefused::Group { group });
        }
        let repeated = bindings[..at].iter().any(|before| before.group == group && before.binding == binding);
        if repeated {
            return Err(ProgramRefused::Repeated { group, binding });
        }
        let sized = match listed.kind {
            BindingKind::Uniform { bytes } => Some((bytes, limits.max_uniform_buffer_binding_size)),
            BindingKind::Storage { bytes, .. } => Some((bytes, limits.max_storage_buffer_binding_size)),
            BindingKind::Texture | BindingKind::Sampler => None,
        };
        if let Some((bytes, most)) = sized {
            let bound = bytes > 0 && u64::from(bytes) <= most;
            if !bound {
                return Err(ProgramRefused::Size { group, binding });
            }
        }
    }
    let most = [
        (BindingKind::Uniform { bytes: 0 }, limits.max_uniform_buffers_per_shader_stage),
        (BindingKind::Storage { bytes: 0, access: Access::Read }, limits.max_storage_buffers_per_shader_stage),
        (BindingKind::Texture, limits.max_sampled_textures_per_shader_stage),
        (BindingKind::Sampler, limits.max_samplers_per_shader_stage),
    ];
    for (kind, most) in most {
        let besides = if kind == BindingKind::Texture {
            textures as usize
        } else {
            0
        };
        let count = bindings.iter().filter(|listed| same_kind(listed.kind, kind)).count() + besides;
        if count > most as usize {
            return Err(ProgramRefused::Many { kind });
        }
    }

    Ok(())
}

/// Checks that `bindings` lists no storage to write: what a draw program or an effect binds, which only reads.
pub(crate) fn check_reads_only(bindings: &[Binding]) -> Result<(), ProgramRefused> {
    let writes =
        bindings.iter().find(|listed| matches!(listed.kind, BindingKind::Storage { access: Access::Write, .. }));
    if let Some(listed) = writes {
        return Err(ProgramRefused::Writes { group: listed.group, binding: listed.binding });
    }

    Ok(())
}

/// Whether two kinds of binding are the same kind, whatever their sizes and access.
const fn same_kind(one: BindingKind, other: BindingKind) -> bool {
    matches!(
        (one, other),
        (BindingKind::Uniform { .. }, BindingKind::Uniform { .. })
            | (BindingKind::Storage { .. }, BindingKind::Storage { .. })
            | (BindingKind::Texture, BindingKind::Texture)
            | (BindingKind::Sampler, BindingKind::Sampler)
    )
}

/// The index of the entry point `name` of `stage`.
pub(crate) fn entry(module: &Module, stage: Stage, name: &str) -> Result<usize, ProgramRefused> {
    let shader = match stage {
        Stage::Vertex => ShaderStage::Vertex,
        Stage::Fragment => ShaderStage::Fragment,
        Stage::Compute => ShaderStage::Compute,
    };
    let found = module.entry_points.iter().position(|entry| entry.stage == shader && entry.name == name);

    found.ok_or_else(|| ProgramRefused::Entry { stage, name: String::from(name) })
}

/// One value passed into or out of a stage at a location: its location, its type, and how it is interpolated.
#[derive(Debug, PartialEq)]
struct Passed<'m> {
    location: u32,
    ty: &'m TypeInner,
    binding: &'m Bound,
}

/// What is passed at locations through `ty`, bound as `binding`: itself, or each member of a struct.
fn passed<'m>(module: &'m Module, ty: naga::Handle<naga::Type>, binding: Option<&'m Bound>) -> Vec<Passed<'m>> {
    let inner = &module.types[ty].inner;
    match (binding, inner) {
        (Some(binding @ Bound::Location { location, .. }), _) => {
            vec![Passed { location: *location, ty: inner, binding }]
        }
        (None, TypeInner::Struct { members, .. }) => {
            members.iter().flat_map(|member| passed(module, member.ty, member.binding.as_ref())).collect()
        }
        _ => Vec::new(),
    }
}

/// What the entry point `at` reads at locations.
fn inputs(module: &Module, at: usize) -> Vec<Passed<'_>> {
    let function = &module.entry_points[at].function;

    function.arguments.iter().flat_map(|argument| passed(module, argument.ty, argument.binding.as_ref())).collect()
}

/// What the entry point `at` writes at locations.
fn outputs(module: &Module, at: usize) -> Vec<Passed<'_>> {
    let function = &module.entry_points[at].function;

    function.result.iter().flat_map(|result| passed(module, result.ty, result.binding.as_ref())).collect()
}

/// The kind of number a value of `ty` holds, a scalar or each of a vector's.
const fn scalar(ty: &TypeInner) -> Option<naga::Scalar> {
    match ty {
        TypeInner::Scalar(scalar) | TypeInner::Vector { scalar, .. } => Some(*scalar),
        _ => None,
    }
}

/// Checks that the vertex stage reads each of its locations from an attribute of either layout, as the kind of number
/// the attribute's format is: of however many of them, as WebGPU fills what a format lacks.
fn check_inputs(
    module: &Module,
    vertex: usize,
    layout: &VertexLayout,
    instances: &PerInstance,
) -> Result<(), ProgramRefused> {
    let records = match instances {
        PerInstance::None => [].as_slice(),
        PerInstance::Records(records) => records.attributes.as_slice(),
    };
    for input in inputs(module, vertex) {
        let location = input.location;
        let given = layout.attributes.iter().chain(records).find(|attribute| attribute.location == location);
        let Some(attribute) = given else {
            return Err(ProgramRefused::Input { location });
        };
        let read = match attribute.format {
            VertexFormat::Uint32 => naga::Scalar::U32,
            VertexFormat::Float32
            | VertexFormat::Float32x2
            | VertexFormat::Float32x3
            | VertexFormat::Float32x4
            | VertexFormat::Unorm8x4 => naga::Scalar::F32,
        };
        let as_format = scalar(input.ty) == Some(read);
        if !as_format {
            return Err(ProgramRefused::InputFormat { location });
        }
    }

    Ok(())
}

/// Checks that the fragment stage reads each of its locations as the vertex stage writes it: the same type, and the
/// same interpolation. Each stage is an entry point of a module, which may be another's.
pub(crate) fn check_stages(
    (vertex_module, vertex): (&Module, usize),
    (fragment_module, fragment): (&Module, usize),
) -> Result<(), ProgramRefused> {
    let written = outputs(vertex_module, vertex);
    for read in inputs(fragment_module, fragment) {
        let matched = written.contains(&read);
        if !matched {
            return Err(ProgramRefused::Stages { location: read.location });
        }
    }

    Ok(())
}

/// Checks that the fragment stage writes the colour drawn into `format` at location 0: a `vec4<f32>`, as a target of
/// fractions or floats is read as four channels and blending by alpha reads the fourth, or a `u32` or a vector of them
/// into whole numbers, of which WebGPU keeps the channels the format has.
pub(crate) fn check_output(module: &Module, fragment: usize, format: ColourFormat) -> Result<(), ProgramRefused> {
    let colour = TypeInner::Vector { size: naga::VectorSize::Quad, scalar: naga::Scalar::F32 };
    let writes = |output: &Passed<'_>| {
        let whole = scalar(output.ty) == Some(naga::Scalar::U32);
        let fits = if format.is_whole() {
            whole
        } else {
            *output.ty == colour
        };
        output.location == 0 && fits
    };
    let written = outputs(module, fragment).iter().any(writes);
    if !written {
        return Err(ProgramRefused::Output);
    }

    Ok(())
}

/// What a binding is used as, as a program lists it, or as an effect reads a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Used {
    /// As a program lists it.
    Listed(BindingKind),
    /// A target an effect reads, of floats, whole numbers, or depth.
    Read(ImageClass),
}

/// Checks that each binding the entry point `at` uses is one `lists` answers for its group and binding, as the kind it
/// is used as, a storage buffer with the access its shader declares. What is read of a block counts an array whose
/// length is its buffer's as one element, as WebGPU's least binding size does.
pub(crate) fn check_uses<U: Into<Used>>(
    module: &Module,
    info: &ModuleInfo,
    at: usize,
    lists: impl Fn(u32, u32) -> Option<U>,
) -> Result<(), ProgramRefused> {
    let uses = info.get_entry_point(at);
    for (handle, global) in module.global_variables.iter() {
        let (Some(place), false) = (&global.binding, uses[handle].is_empty()) else {
            continue;
        };
        let (group, binding) = (place.group, place.binding);
        let Some(listed) = lists(group, binding) else {
            return Err(ProgramRefused::Unbound { group, binding });
        };
        let inner = &module.types[global.ty].inner;
        let float_texture = ImageClass::Sampled { kind: ScalarKind::Float, multi: false };
        let fits = match (global.space, inner, listed.into()) {
            (AddressSpace::Uniform, _, Used::Listed(BindingKind::Uniform { bytes })) => {
                inner.size(module.to_ctx()) <= bytes
            }
            (AddressSpace::Storage { access: declared }, _, Used::Listed(BindingKind::Storage { bytes, access })) => {
                let declared = if declared.contains(StorageAccess::STORE) {
                    Access::Write
                } else {
                    Access::Read
                };
                declared == access && inner.size(module.to_ctx()) <= bytes
            }
            (AddressSpace::Handle, TypeInner::Image { dim, arrayed, class }, Used::Listed(BindingKind::Texture)) => {
                *dim == ImageDimension::D2 && !*arrayed && *class == float_texture
            }
            (AddressSpace::Handle, TypeInner::Image { dim, arrayed, class }, Used::Read(read)) => {
                *dim == ImageDimension::D2 && !*arrayed && *class == read
            }
            (AddressSpace::Handle, TypeInner::Sampler { comparison }, Used::Listed(BindingKind::Sampler)) => {
                !*comparison
            }
            _ => false,
        };
        if !fits {
            return Err(ProgramRefused::Mismatch { group, binding });
        }
    }

    Ok(())
}

impl From<BindingKind> for Used {
    fn from(kind: BindingKind) -> Self {
        Self::Listed(kind)
    }
}
