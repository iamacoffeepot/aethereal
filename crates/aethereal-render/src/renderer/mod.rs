//! The renderer: the root that owns the GPU, makes what actors ask it for, and draws each frame (ADR-0021).

use crate::Gpu;
use crate::instances::{self, CreateInstances, Held, Instanced, InstancesId, WriteInstances};
use crate::made::{Ids, Made, Makers, Owned};
use crate::mesh::{self, Counted, CreateMesh, MeshId, Meshed, VertexLayout};
use crate::program::{
    self, Binding, BindingKind, Blend, CreateProgram, Cull, Depth, PerInstance, ProgramId, Programmed,
};
use crate::texture::{self, CreateTexture, TextureId, Textured};
use crate::uniforms::{self, CreateUniforms, Uniformed, UniformsId, WriteUniforms};
use aethereal_actor::{
    Actor, Beginning, Carries, ChildOf, Ended, Ending, Fault, Given, Handle, Handles, Level, On, Protocol, Render,
    Resource, ToSender, Top, Turn,
};
use aethereal_data::{ActorId, Bound, Kind, KindName, Plain};

/// The renderer, the one actor that touches the GPU. Actors ask it through [`Rendering`].
#[derive(Bound)]
pub struct Renderer {
    gpu: Handle<Gpu>,
    kept: Handle<Kept>,
}

impl Kind for Renderer {
    const NAME: KindName = KindName::new("aether.render");
}

/// The renderer is a root, made by the chassis that opened the GPU (ADR-0021, ADR-0022).
impl ChildOf for Renderer {
    type Parent = Top;
}

/// What asking the renderer at its typed path, `path::<Renderer>(())`, gets: as asking it through [`Rendering`] does
/// (ADR-0022).
impl Carries<CreateMesh> for Renderer {
    type Answers = (Meshed,);
}

impl Carries<CreateTexture> for Renderer {
    type Answers = (Textured,);
}

impl Carries<CreateProgram> for Renderer {
    type Answers = (Programmed,);
}

impl Carries<CreateInstances> for Renderer {
    type Answers = (Instanced,);
}

impl Carries<CreateUniforms> for Renderer {
    type Answers = (Uniformed,);
}

/// What the renderer is made from: the GPU the chassis opened.
#[derive(Bound)]
pub struct RendererConfig {
    pub gpu: Given<Gpu>,
}

impl Kind for RendererConfig {
    const NAME: KindName = KindName::new("aether.render.config");
}

/// What an actor asks the renderer: to make a mesh, a texture, a program, instances, or uniforms, to write instances
/// or uniforms it made, or to release what it made.
pub struct Rendering;

impl Protocol for Rendering {
    type Kinds = (
        CreateMesh,
        CreateTexture,
        CreateProgram,
        CreateInstances,
        WriteInstances,
        CreateUniforms,
        WriteUniforms,
        Release,
    );
}

impl Carries<CreateMesh> for Rendering {
    type Answers = (Meshed,);
}

impl Carries<CreateTexture> for Rendering {
    type Answers = (Textured,);
}

impl Carries<CreateProgram> for Rendering {
    type Answers = (Programmed,);
}

impl Carries<CreateInstances> for Rendering {
    type Answers = (Instanced,);
}

impl Carries<WriteInstances> for Rendering {
    type Answers = ();
}

impl Carries<CreateUniforms> for Rendering {
    type Answers = (Uniformed,);
}

impl Carries<WriteUniforms> for Rendering {
    type Answers = ();
}

impl Carries<Release> for Rendering {
    type Answers = ();
}

/// Releases what the renderer made before its maker ends. Only its maker may; what another asks is logged and left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Release {
    Mesh(MeshId),
    Texture(TextureId),
    Program(ProgramId),
    Instances(InstancesId),
    Uniforms(UniformsId),
}

impl Kind for Release {
    const NAME: KindName = KindName::new("aether.render.release");
}

/// What the renderer has made, and who made each.
#[derive(Default)]
struct Kept {
    ids: Ids,
    makers: Makers,
    meshes: Made<Mesh>,
    textures: Made<Texture>,
    programs: Made<Program>,
    instances: Made<Instances>,
    uniforms: Made<Uniforms>,
}

impl Resource for Kept {}

/// A mesh on the GPU.
#[expect(dead_code, reason = "drawn once the renderer draws what actors place (ADR-0021)")]
struct Mesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    counted: Counted,
    layout: VertexLayout,
}

/// A texture on the GPU, and the view a program samples it through.
#[expect(dead_code, reason = "drawn with once programs bind textures (ADR-0021)")]
struct Texture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// A program on the GPU: its pipeline, with the layout of the vertices it reads and of each group it binds.
#[expect(dead_code, reason = "drawn with once the renderer draws what actors place (ADR-0021)")]
struct Program {
    pipeline: wgpu::RenderPipeline,
    layout: VertexLayout,
    instances: PerInstance,
    groups: Vec<wgpu::BindGroupLayout>,
}

/// Instances on the GPU: a buffer with room for their capacity, how they stand, and how a record is laid out.
struct Instances {
    buffer: wgpu::Buffer,
    held: Held,
    #[expect(dead_code, reason = "matched against a program's once actors place what they draw (ADR-0023)")]
    layout: VertexLayout,
}

/// Uniforms on the GPU, and their size in bytes.
struct Uniforms {
    buffer: wgpu::Buffer,
    size: u32,
}

impl Actor for Renderer {
    type Config = RendererConfig;
    type Key = ();
    type Handles = (
        On<Render>,
        CreateMesh,
        CreateTexture,
        CreateProgram,
        CreateInstances,
        WriteInstances,
        CreateUniforms,
        WriteUniforms,
        Release,
        Ended,
    );
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: RendererConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let gpu = turn.take(config.gpu)?;
        let kept = turn.keep(Kept::default())?;
        turn.subscribe::<Render>();

        Ok(Self { gpu, kept })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Render>> for Renderer {
    type Sends = ();

    fn handle(&mut self, _: Render, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let gpu = turn.open(&mut self.gpu)?;
        // A frame the surface has no texture for is skipped; the next asks again.
        let wgpu::CurrentSurfaceTexture::Success(frame) = gpu.surface().get_current_texture() else {
            return Ok(());
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu.device().create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let attachment = wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
        };
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(attachment)],
            ..Default::default()
        }));
        gpu.queue().submit([encoder.finish()]);
        gpu.queue().present(frame);

        Ok(())
    }
}

impl Handles<CreateMesh> for Renderer {
    type Sends = (ToSender<Meshed>,);

    fn handle(&mut self, create: CreateMesh, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        // Copied out of the store: the bytes are read through the turn, as the GPU is opened through it.
        let vertices = turn.read(&create.vertices).to_vec();
        let indices = turn.read(&create.indices).to_vec();
        let counted = match mesh::check(&create.layout, &vertices, &indices) {
            Ok(counted) => counted,
            Err(why) => {
                turn.reply(&Meshed::Refused(why))?;
                return Ok(());
            }
        };

        let gpu = turn.open(&mut self.gpu)?;
        let mesh = Mesh {
            vertices: upload(gpu, &vertices, wgpu::BufferUsages::VERTEX),
            indices: upload(gpu, &indices, wgpu::BufferUsages::INDEX),
            counted,
            layout: create.layout,
        };
        let id = self.made(turn, |kept, id, maker| kept.meshes.keep(id, maker, mesh))?;
        turn.reply(&Meshed::Made(MeshId { id }))?;

        Ok(())
    }
}

impl Handles<CreateTexture> for Renderer {
    type Sends = (ToSender<Textured>,);

    fn handle(&mut self, create: CreateTexture, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let CreateTexture { width, height, format, pixels } = create;
        let pixels = turn.read(&pixels).to_vec();
        let most = turn.open(&mut self.gpu)?.device().limits().max_texture_dimension_2d;
        if let Err(why) = texture::check(width, height, format, &pixels, most) {
            turn.reply(&Textured::Refused(why))?;
            return Ok(());
        }

        let gpu = turn.open(&mut self.gpu)?;
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let made = gpu.device().create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: format.wgpu(),
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let layout = wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * format.bytes()),
            rows_per_image: Some(height),
        };
        gpu.queue().write_texture(made.as_image_copy(), &pixels, layout, size);
        let view = made.create_view(&wgpu::TextureViewDescriptor::default());
        let texture = Texture { texture: made, view };
        let id = self.made(turn, |kept, id, maker| kept.textures.keep(id, maker, texture))?;
        turn.reply(&Textured::Made(TextureId { id }))?;

        Ok(())
    }
}

impl Handles<CreateProgram> for Renderer {
    type Sends = (ToSender<Programmed>,);

    fn handle(&mut self, create: CreateProgram, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let limits = turn.open(&mut self.gpu)?.device().limits();
        if let Err(why) = program::check(&create, &limits) {
            turn.reply(&Programmed::Refused(why))?;
            return Ok(());
        }

        let gpu = turn.open(&mut self.gpu)?;
        let made = pipeline(gpu, &create);
        let id = self.made(turn, |kept, id, maker| kept.programs.keep(id, maker, made))?;
        turn.reply(&Programmed::Made(ProgramId { id }))?;

        Ok(())
    }
}

impl Handles<CreateInstances> for Renderer {
    type Sends = (ToSender<Instanced>,);

    fn handle(&mut self, create: CreateInstances, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let CreateInstances { layout, capacity, records } = create;
        let records = turn.read(&records).to_vec();
        let most = turn.open(&mut self.gpu)?.device().limits().max_buffer_size;
        let length = match instances::check(&layout, capacity, &records, most) {
            Ok(length) => length,
            Err(why) => {
                turn.reply(&Instanced::Refused(why))?;
                return Ok(());
            }
        };

        let gpu = turn.open(&mut self.gpu)?;
        let room = u64::from(capacity) * u64::from(layout.stride);
        let buffer = gpu.device().create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: room,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue().write_buffer(&buffer, 0, &records);
        let held = Held { stride: layout.stride, capacity, length };
        let made = Instances { buffer, held, layout };
        let id = self.made(turn, |kept, id, maker| kept.instances.keep(id, maker, made))?;
        turn.reply(&Instanced::Made(InstancesId { id }))?;

        Ok(())
    }
}

impl Handles<WriteInstances> for Renderer {
    type Sends = ();

    fn handle(&mut self, write: WriteInstances, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let by = turn.sender().id();
        let records = turn.read(&write.records).to_vec();
        let kept = turn.open(&mut self.kept)?;
        let written = kept.instances.owned(write.instances.id, by).map(|instances| {
            let length = instances.held.write(write.first, records.len() as u64);
            if let Ok(length) = length {
                instances.held.length = length;
            }
            length.map(|_| (instances.buffer.clone(), instances.held.stride))
        });
        let (buffer, stride) = match written {
            Owned::Mine(Ok(written)) => written,
            Owned::Mine(Err(why)) => {
                turn.log(Level::Warn, format_args!("{:?} was not written: {why:?}", write.instances));
                return Ok(());
            }
            Owned::NotOwner => {
                turn.log(Level::Warn, format_args!("{:?} was not written: another made it", write.instances));
                return Ok(());
            }
            Owned::Unknown => {
                turn.log(Level::Warn, format_args!("{:?} was not written: no such thing", write.instances));
                return Ok(());
            }
        };

        let at = u64::from(write.first) * u64::from(stride);
        turn.open(&mut self.gpu)?.queue().write_buffer(&buffer, at, &records);

        Ok(())
    }
}

impl Handles<CreateUniforms> for Renderer {
    type Sends = (ToSender<Uniformed>,);

    fn handle(&mut self, create: CreateUniforms, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let bytes = turn.read(&create.bytes).to_vec();
        let most = turn.open(&mut self.gpu)?.device().limits().max_uniform_buffer_binding_size;
        let size = match uniforms::check(&bytes, most) {
            Ok(size) => size,
            Err(why) => {
                turn.reply(&Uniformed::Refused(why))?;
                return Ok(());
            }
        };

        let gpu = turn.open(&mut self.gpu)?;
        let buffer = upload(gpu, &bytes, wgpu::BufferUsages::UNIFORM);
        let made = Uniforms { buffer, size };
        let id = self.made(turn, |kept, id, maker| kept.uniforms.keep(id, maker, made))?;
        turn.reply(&Uniformed::Made(UniformsId { id }))?;

        Ok(())
    }
}

impl Handles<WriteUniforms> for Renderer {
    type Sends = ();

    fn handle(&mut self, write: WriteUniforms, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let by = turn.sender().id();
        let bytes = turn.read(&write.bytes).to_vec();
        let kept = turn.open(&mut self.kept)?;
        let written = kept.uniforms.owned(write.uniforms.id, by).map(|uniforms| {
            let fits = uniforms::writes(uniforms.size, write.first, bytes.len() as u64);
            (fits, uniforms.buffer.clone())
        });
        let buffer = match written {
            Owned::Mine((true, buffer)) => buffer,
            Owned::Mine((false, _)) => {
                turn.log(Level::Warn, format_args!("{:?} was not written: it lies outside them", write.uniforms));
                return Ok(());
            }
            Owned::NotOwner => {
                turn.log(Level::Warn, format_args!("{:?} was not written: another made them", write.uniforms));
                return Ok(());
            }
            Owned::Unknown => {
                turn.log(Level::Warn, format_args!("{:?} was not written: no such thing", write.uniforms));
                return Ok(());
            }
        };

        turn.open(&mut self.gpu)?.queue().write_buffer(&buffer, u64::from(write.first), &bytes);

        Ok(())
    }
}

impl Handles<Release> for Renderer {
    type Sends = ();

    fn handle(&mut self, release: Release, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let by = turn.sender().id();
        let kept = turn.open(&mut self.kept)?;
        let released = match release {
            Release::Mesh(mesh) => kept.meshes.release(mesh.id, by).map(drop),
            Release::Texture(texture) => kept.textures.release(texture.id, by).map(drop),
            Release::Program(program) => kept.programs.release(program.id, by).map(drop),
            Release::Instances(instances) => kept.instances.release(instances.id, by).map(drop),
            Release::Uniforms(uniforms) => kept.uniforms.release(uniforms.id, by).map(drop),
        };
        match released {
            Owned::Mine(()) => {}
            Owned::NotOwner => turn.log(Level::Warn, format_args!("{release:?} was not released: another made it")),
            Owned::Unknown => turn.log(Level::Warn, format_args!("{release:?} was not released: no such thing")),
        }

        Ok(())
    }
}

impl Handles<Ended> for Renderer {
    type Sends = ();

    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let maker = ended.actor().id();
        let kept = turn.open(&mut self.kept)?;
        let watched = kept.makers.ended(maker);
        if watched {
            kept.meshes.release_all(maker);
            kept.textures.release_all(maker);
            kept.programs.release_all(maker);
            kept.instances.release_all(maker);
            kept.uniforms.release_all(maker);
        }

        Ok(())
    }
}

impl Renderer {
    /// Keeps what was just made for the sender of the mail being handled, under a new id, and watches the sender
    /// when it is the first thing it has made, so what it made is released when it ends. Answers the id.
    fn made<S>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        keep: impl FnOnce(&mut Kept, u64, ActorId),
    ) -> Result<u64, Fault> {
        let maker = turn.sender();
        let kept = turn.open(&mut self.kept)?;
        let id = kept.ids.next();
        keep(kept, id, maker.id());
        let first = kept.makers.made(maker.id());
        if first {
            turn.watch(maker);
        }

        Ok(id)
    }
}

/// A buffer of `usage` on the GPU, holding `bytes`.
fn upload(gpu: &Gpu, bytes: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
    let buffer = gpu.device().create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes.len() as u64,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue().write_buffer(&buffer, 0, bytes);

    buffer
}

/// The depth a program tests against: what the pass it draws in holds beside its colour.
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The pipeline `create` asks for, drawing to the surface's format, made from what its check let through.
fn pipeline(gpu: &Gpu, create: &CreateProgram) -> Program {
    let device = gpu.device();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(create.wgsl.as_str().into()),
    });
    let groups = groups(device, &create.bindings);
    let bound: Vec<Option<&wgpu::BindGroupLayout>> = groups.iter().map(Some).collect();
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &bound,
        immediate_size: 0,
    });
    let attributes = attributes_of(&create.layout);
    let vertices = wgpu::VertexBufferLayout {
        array_stride: u64::from(create.layout.stride),
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &attributes,
    };
    let per_instance = match &create.instances {
        PerInstance::None => Vec::new(),
        PerInstance::Records(records) => attributes_of(records),
    };
    // A second buffer only for a program that reads one: wgpu takes the buffers a pipeline reads as a list.
    let instances = match &create.instances {
        PerInstance::None => None,
        PerInstance::Records(records) => Some(wgpu::VertexBufferLayout {
            array_stride: u64::from(records.stride),
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &per_instance,
        }),
    };
    let blend = match create.state.blend {
        Blend::Opaque => None,
        Blend::Alpha => Some(wgpu::BlendState::ALPHA_BLENDING),
        Blend::Additive => {
            let add = wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            };
            Some(wgpu::BlendState { color: add, alpha: add })
        }
    };
    let (compare, writes) = match create.state.depth {
        Depth::Off => (wgpu::CompareFunction::Always, false),
        Depth::Test => (wgpu::CompareFunction::Less, false),
        Depth::TestWrite => (wgpu::CompareFunction::Less, true),
    };
    let cull = match create.state.cull {
        Cull::None => None,
        Cull::Back => Some(wgpu::Face::Back),
        Cull::Front => Some(wgpu::Face::Front),
    };
    let target = wgpu::ColorTargetState { format: gpu.format(), blend, write_mask: wgpu::ColorWrites::ALL };
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some(&create.vertex),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(vertices), instances],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: cull,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH,
            depth_write_enabled: Some(writes),
            depth_compare: Some(compare),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(&create.fragment),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(target)],
        }),
        multiview_mask: None,
        cache: None,
    });

    Program { pipeline, layout: create.layout.clone(), instances: create.instances.clone(), groups }
}

/// The attributes of `layout`, as wgpu names them.
fn attributes_of(layout: &VertexLayout) -> Vec<wgpu::VertexAttribute> {
    layout
        .attributes
        .iter()
        .map(|attribute| wgpu::VertexAttribute {
            format: attribute.format.wgpu(),
            offset: u64::from(attribute.offset),
            shader_location: attribute.location,
        })
        .collect()
}

/// The layout of each group `bindings` binds in, from the first to the last of them, an empty one for a group they
/// skip; each binding is seen by both stages.
fn groups(device: &wgpu::Device, bindings: &[Binding]) -> Vec<wgpu::BindGroupLayout> {
    let count = bindings.iter().map(|binding| binding.group + 1).max().unwrap_or(0);

    (0..count)
        .map(|group| {
            let entries: Vec<wgpu::BindGroupLayoutEntry> = bindings
                .iter()
                .filter(|binding| binding.group == group)
                .map(|binding| wgpu::BindGroupLayoutEntry {
                    binding: binding.binding,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: binding_type(binding.kind),
                    count: None,
                })
                .collect();
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &entries })
        })
        .collect()
}

/// What a binding of `kind` is, as wgpu names it.
fn binding_type(kind: BindingKind) -> wgpu::BindingType {
    match kind {
        BindingKind::Uniform { bytes } => wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: wgpu::BufferSize::new(u64::from(bytes)),
        },
        BindingKind::Texture => wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        BindingKind::Sampler => wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
    }
}
