//! The renderer: the root that owns the GPU, makes what actors ask it for, and draws each frame (ADR-0021).

use crate::Gpu;
use crate::instances::{self, CreateInstances, Instanced, InstancesId, WriteInstances};
use crate::made::{Ids, Made, Makers, Owned};
use crate::mapping::{Mapped, Mapping};
use crate::mesh::{self, Counted, CreateMesh, MeshId, Meshed, VertexLayout};
use crate::placement::{
    self, Drawn, Filter, Finds, Place, Placed, PlacementId, PlacementRefused, Sampling, Shape, Wrap,
};
use crate::program::{
    self, Binding, BindingKind, Blend, CreateProgram, Cull, Depth, PerInstance, ProgramId, Programmed,
};
use crate::readback::{self, Copied, Order, Pixels, ReadBack, ReadRefused, Readable};
use crate::storage::{self, CreateStorage, StorageId, Stored, WriteStorage};
use crate::texture::{self, CreateTexture, TextureFormat, TextureId, Textured};
use crate::timing::{self, TimeFrame, Timed, TimingRefused};
use crate::uniforms::{self, CreateUniforms, Uniformed, UniformsId, WriteUniforms};
use aethereal_actor::{
    Actor, Beginning, Carries, ChildOf, Controller, Ended, Ending, Fault, Given, Handle, Handles, Held, Level, On,
    Protocol, Render, Resource, Source, ToSender, Top, Turn,
};
use aethereal_data::{ActorId, Bound, Kind, KindName, Plain};
use std::collections::HashMap;
use std::mem;

/// The renderer, the one actor that touches the GPU. Actors ask it through [`Rendering`].
#[derive(Bound)]
pub struct Renderer {
    gpu: Handle<Gpu>,
    kept: Handle<Kept>,
    mapping: Handle<Controller<Mapping>>,
    /// Held so the chassis's word of what it mapped is brought here; nothing opens it.
    mapped: Handle<Source<Mapped>>,
    owed: Owed,
}

/// The reads and timings the renderer owes: those of the next frame, and those copied and waiting on their buffer's
/// map, each under the read the buffer is kept by.
#[derive(Bound)]
struct Owed {
    frames: Vec<Held<Pixels>>,
    timings: Vec<Held<Timed>>,
    reading: Vec<(u64, Held<Pixels>)>,
    timing: Vec<(u64, Held<Timed>)>,
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

impl Carries<CreateStorage> for Renderer {
    type Answers = (Stored,);
}

impl Carries<Place> for Renderer {
    type Answers = (Placed,);
}

impl Carries<ReadBack> for Renderer {
    type Answers = (Pixels,);
}

impl Carries<TimeFrame> for Renderer {
    type Answers = (Timed,);
}

/// What the renderer is made from: the GPU the chassis opened, the controller it asks the chassis to map what it
/// reads back through, and the source the chassis brings word of each through (ADR-0019).
#[derive(Bound)]
pub struct RendererConfig {
    pub gpu: Given<Gpu>,
    pub mapping: Given<Controller<Mapping>>,
    pub mapped: Given<Source<Mapped>>,
}

impl Kind for RendererConfig {
    const NAME: KindName = KindName::new("aether.render.config");
}

/// What an actor asks the renderer: to make a mesh, a texture, a program, instances, uniforms, or storage, to write
/// instances, uniforms, or storage it made, to place what is drawn, to release what it made, or to read back or time
/// what it draws.
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
        CreateStorage,
        WriteStorage,
        Place,
        Release,
        ReadBack,
        TimeFrame,
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

impl Carries<CreateStorage> for Rendering {
    type Answers = (Stored,);
}

impl Carries<WriteStorage> for Rendering {
    type Answers = ();
}

impl Carries<Place> for Rendering {
    type Answers = (Placed,);
}

impl Carries<Release> for Rendering {
    type Answers = ();
}

impl Carries<ReadBack> for Rendering {
    type Answers = (Pixels,);
}

impl Carries<TimeFrame> for Rendering {
    type Answers = (Timed,);
}

/// Releases what the renderer made before its maker ends. Only its maker may; what another asks is logged and left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Release {
    Mesh(MeshId),
    Texture(TextureId),
    Program(ProgramId),
    Instances(InstancesId),
    Uniforms(UniformsId),
    Storage(StorageId),
    Placement(PlacementId),
}

impl Kind for Release {
    const NAME: KindName = KindName::new("aether.render.release");
}

/// What the renderer has made, and who made each, with the samplers its placements share and the depth its frames
/// test against.
struct Kept {
    ids: Ids,
    makers: Makers,
    meshes: Made<Mesh>,
    textures: Made<Texture>,
    programs: Made<Program>,
    instances: Made<Instances>,
    uniforms: Made<Uniforms>,
    storage: Made<Storage>,
    placements: Made<Placement>,
    reads: HashMap<u64, Reading>,
    samplers: HashMap<Sampling, wgpu::Sampler>,
    depth: DepthTexture,
}

impl Resource for Kept {}

/// A mesh on the GPU.
struct Mesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    counted: Counted,
    layout: VertexLayout,
}

/// A texture on the GPU, the view a program samples it through, and its format, which it is read back in.
struct Texture {
    made: wgpu::Texture,
    view: wgpu::TextureView,
    format: TextureFormat,
}

/// A buffer the GPU copied into, waiting for the chassis to map it, and how its bytes are read once mapped.
enum Reading {
    /// Pixels, copied so, of `format`, their channels in `order`.
    Pixels { buffer: wgpu::Buffer, copied: Copied, format: TextureFormat, order: Order },
    /// A pass's timestamps, each tick `period` nanoseconds.
    Stamps { buffer: wgpu::Buffer, period: f32 },
}

/// A program on the GPU: its pipeline, with the layouts of what it reads, what it binds, and the layout of each
/// group it binds in.
struct Program {
    pipeline: wgpu::RenderPipeline,
    layout: VertexLayout,
    instances: PerInstance,
    bindings: Vec<Binding>,
    groups: Vec<wgpu::BindGroupLayout>,
}

/// Instances on the GPU: a buffer with room for their capacity, how they stand, and how a record is laid out.
struct Instances {
    buffer: wgpu::Buffer,
    held: instances::Held,
    layout: VertexLayout,
}

/// One thing drawn at each frame, holding all it draws with, so a frame looks nothing up and releasing what it was
/// made of frees it only once the placement goes (ADR-0023). `program` is its program's id, which a frame draws
/// placements together by.
struct Placement {
    program: u64,
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    drawing: Drawing,
    groups: Vec<wgpu::BindGroup>,
}

/// How many instances a placement draws.
enum Drawing {
    /// A count, fixed when it was placed.
    Count(u32),
    /// One for each record of the instances `instances`, read from `buffer`.
    Records { instances: u64, buffer: wgpu::Buffer, length: Length },
}

/// How many records a placement draws from its instances.
#[derive(Clone, Copy)]
enum Length {
    /// As many as the instances hold at the frame: they are still kept, and written.
    Live,
    /// As many as they held when they were released: nothing writes them again.
    Last(u32),
}

/// The depth texture a frame tests against, of the size of the frame it was last made for.
struct DepthTexture {
    view: wgpu::TextureView,
    width: u32,
    height: u32,
}

/// Uniforms on the GPU, and their size in bytes.
struct Uniforms {
    buffer: wgpu::Buffer,
    size: u32,
}

/// Storage on the GPU, and its size in bytes.
struct Storage {
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
        CreateStorage,
        WriteStorage,
        Place,
        Release,
        ReadBack,
        TimeFrame,
        Mapped,
        Ended,
    );
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: RendererConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let mut gpu = turn.take(config.gpu)?;
        let mapping = turn.take(config.mapping)?;
        let mapped = turn.take(config.mapped)?;
        // Made again at the first frame's size, as for each frame of a new size.
        let depth = DepthTexture::new(turn.open(&mut gpu)?.device(), 1, 1);
        let kept = Kept {
            ids: Ids::default(),
            makers: Makers::default(),
            meshes: Made::default(),
            textures: Made::default(),
            programs: Made::default(),
            instances: Made::default(),
            uniforms: Made::default(),
            storage: Made::default(),
            placements: Made::default(),
            reads: HashMap::new(),
            samplers: HashMap::new(),
            depth,
        };
        let kept = turn.keep(kept)?;
        turn.subscribe::<Render>();

        let owed = Owed { frames: Vec::new(), timings: Vec::new(), reading: Vec::new(), timing: Vec::new() };

        Ok(Self { gpu, kept, mapping, mapped, owed })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Render>> for Renderer {
    type Sends = ();

    fn handle(&mut self, _: Render, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let gpu = turn.open(&mut self.gpu)?;
        // A frame the surface has no texture for is skipped; the next asks again, and is the one read and timed.
        let wgpu::CurrentSurfaceTexture::Success(frame) = gpu.surface().get_current_texture() else {
            return Ok(());
        };
        // Held past the opening of what is kept, as a turn opens one resource at a time.
        let (device, queue) = (gpu.device().clone(), gpu.queue().clone());
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let kept = turn.open(&mut self.kept)?;
        kept.depth.fit(&device, frame.texture.width(), frame.texture.height());

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let stamps = (!self.owed.timings.is_empty()).then(|| Stamps::new(&device));
        let attachment = wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
        };
        let depth = wgpu::RenderPassDepthStencilAttachment {
            view: &kept.depth.view,
            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
            stencil_ops: None,
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(attachment)],
            depth_stencil_attachment: Some(depth),
            timestamp_writes: stamps.as_ref().map(Stamps::writes),
            ..Default::default()
        });
        kept.draw(&mut pass);
        drop(pass);
        let copied = (!self.owed.frames.is_empty()).then(|| copy_frame(&device, &mut encoder, &frame.texture));
        let timed = stamps.map(|stamps| stamps.resolve(&mut encoder, queue.get_timestamp_period()));
        queue.submit([encoder.finish()]);
        queue.present(frame);

        let mut mappings = Vec::new();
        let mut refused = Vec::new();
        match copied {
            Some(Ok(reading)) => {
                let (read, mapping) = kept.reading(reading);
                mappings.push(mapping);
                let frames = mem::take(&mut self.owed.frames);
                self.owed.reading.extend(frames.into_iter().map(|held| (read, held)));
            }
            Some(Err(why)) => refused.extend(mem::take(&mut self.owed.frames).into_iter().map(|held| (held, why))),
            None => {}
        }
        if let Some(reading) = timed {
            let (read, mapping) = kept.reading(reading);
            mappings.push(mapping);
            let timings = mem::take(&mut self.owed.timings);
            self.owed.timing.extend(timings.into_iter().map(|held| (read, held)));
        }
        for (held, why) in refused {
            turn.answer(held, &Pixels::Refused(why))?;
        }
        // Pushed last, so a turn that fails before it asks the chassis for nothing (ADR-0019).
        let control = turn.open(&mut self.mapping)?;
        for mapping in mappings {
            control.push(mapping);
        }

        Ok(())
    }
}

impl Handles<ReadBack> for Renderer {
    type Sends = (ToSender<Pixels>,);

    fn handle(&mut self, read: ReadBack, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Readable::Texture(texture) = read.of else {
            // Read when the next frame is drawn.
            let held = turn.hold()?;
            self.owed.frames.push(held);
            return Ok(());
        };
        let gpu = turn.open(&mut self.gpu)?;
        let (device, queue) = (gpu.device().clone(), gpu.queue().clone());
        let kept = turn.open(&mut self.kept)?;
        let Some((made, format)) = kept.textures.get(texture.id).map(|kept| (kept.made.clone(), kept.format)) else {
            turn.reply(&Pixels::Refused(ReadRefused::Texture))?;
            return Ok(());
        };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let reading = copy_texture(&device, &mut encoder, &made, format, Order::Kept);
        queue.submit([encoder.finish()]);
        let (read, mapping) = kept.reading(reading);
        let held = turn.hold()?;
        self.owed.reading.push((read, held));
        turn.open(&mut self.mapping)?.push(mapping);

        Ok(())
    }
}

impl Handles<TimeFrame> for Renderer {
    type Sends = (ToSender<Timed>,);

    fn handle(&mut self, _: TimeFrame, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let features = turn.open(&mut self.gpu)?.device().features();
        if !features.contains(wgpu::Features::TIMESTAMP_QUERY) {
            turn.reply(&Timed::Refused(TimingRefused::Unsupported))?;
            return Ok(());
        }
        // Timed when the next frame is drawn.
        let held = turn.hold()?;
        self.owed.timings.push(held);

        Ok(())
    }
}

impl Handles<Mapped> for Renderer {
    type Sends = ();

    fn handle(&mut self, mapped: Mapped, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        // What the source brings comes from this actor to itself; a buffer is read only once the chassis says it is
        // mapped, as reading one that is not stops the program.
        let brought = turn.sender().id() == turn.me().id();
        if !brought {
            turn.log(Level::Warn, format_args!("{mapped:?} was not read: only the chassis says what it mapped"));
            return Ok(());
        }
        let (read, ready) = match mapped {
            Mapped::Ready { read } => (read, true),
            Mapped::Failed { read } => (read, false),
        };
        let kept = turn.open(&mut self.kept)?;
        let Some(reading) = kept.reads.remove(&read) else {
            turn.log(Level::Warn, format_args!("{mapped:?} was not read: no such read"));
            return Ok(());
        };

        match reading {
            Reading::Pixels { buffer, copied, format, order } => {
                let pixels = ready.then(|| mapped_bytes(&buffer, |bytes| copied.unpad(bytes, order))).flatten();
                let answer = match pixels.map(|pixels| turn.put(pixels)) {
                    Some(Ok(pixels)) => Pixels::Read { width: copied.width, height: copied.height, format, pixels },
                    Some(Err(_)) => Pixels::Refused(ReadRefused::Unstored),
                    None => Pixels::Refused(ReadRefused::Unmapped),
                };
                for held in owed_by(&mut self.owed.reading, read) {
                    turn.answer(held, &answer)?;
                }
            }
            Reading::Stamps { buffer, period } => {
                let passes = ready.then(|| mapped_bytes(&buffer, |bytes| timing::passes(bytes, period))).flatten();
                let answer = passes.map_or(Timed::Refused(TimingRefused::Unmapped), |passes| Timed::Took { passes });
                for held in owed_by(&mut self.owed.timing, read) {
                    turn.answer(held, &answer)?;
                }
            }
        }

        Ok(())
    }
}

/// What `read` owes of `owed`, taken from it.
fn owed_by<A>(owed: &mut Vec<(u64, Held<A>)>, read: u64) -> Vec<Held<A>> {
    owed.extract_if(.., |(owing, _)| *owing == read).map(|(_, held)| held).collect()
}

/// What `read` makes of the bytes of `buffer`, which the chassis has mapped, before it is unmapped: none when the GPU
/// will not show them.
fn mapped_bytes<T>(buffer: &wgpu::Buffer, read: impl FnOnce(&[u8]) -> T) -> Option<T> {
    let made = buffer.get_mapped_range(..).ok().map(|view| read(&view));
    buffer.unmap();

    made
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
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let layout = wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * format.bytes()),
            rows_per_image: Some(height),
        };
        gpu.queue().write_texture(made.as_image_copy(), &pixels, layout, size);
        let view = made.create_view(&wgpu::TextureViewDescriptor::default());
        let texture = Texture { made, view, format };
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
        let held = instances::Held { stride: layout.stride, capacity, length };
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

impl Handles<CreateStorage> for Renderer {
    type Sends = (ToSender<Stored>,);

    fn handle(&mut self, create: CreateStorage, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let bytes = turn.read(&create.bytes).to_vec();
        let most = turn.open(&mut self.gpu)?.device().limits().max_storage_buffer_binding_size;
        let size = match storage::check(&bytes, most) {
            Ok(size) => size,
            Err(why) => {
                turn.reply(&Stored::Refused(why))?;
                return Ok(());
            }
        };

        let gpu = turn.open(&mut self.gpu)?;
        let buffer = upload(gpu, &bytes, wgpu::BufferUsages::STORAGE);
        let made = Storage { buffer, size };
        let id = self.made(turn, |kept, id, maker| kept.storage.keep(id, maker, made))?;
        turn.reply(&Stored::Made(StorageId { id }))?;

        Ok(())
    }
}

impl Handles<WriteStorage> for Renderer {
    type Sends = ();

    fn handle(&mut self, write: WriteStorage, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let by = turn.sender().id();
        let bytes = turn.read(&write.bytes).to_vec();
        let kept = turn.open(&mut self.kept)?;
        let written = kept.storage.owned(write.storage.id, by).map(|storage| {
            let fits = storage::writes(storage.size, write.first, bytes.len() as u64);
            (fits, storage.buffer.clone())
        });
        let buffer = match written {
            Owned::Mine((true, buffer)) => buffer,
            Owned::Mine((false, _)) => {
                turn.log(Level::Warn, format_args!("{:?} was not written: it lies outside it", write.storage));
                return Ok(());
            }
            Owned::NotOwner => {
                turn.log(Level::Warn, format_args!("{:?} was not written: another made it", write.storage));
                return Ok(());
            }
            Owned::Unknown => {
                turn.log(Level::Warn, format_args!("{:?} was not written: no such thing", write.storage));
                return Ok(());
            }
        };

        turn.open(&mut self.gpu)?.queue().write_buffer(&buffer, u64::from(write.first), &bytes);

        Ok(())
    }
}

impl Handles<Place> for Renderer {
    type Sends = (ToSender<Placed>,);

    fn handle(&mut self, place: Place, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let device = turn.open(&mut self.gpu)?.device().clone();
        let kept = turn.open(&mut self.kept)?;
        let placed = placement::check(&place, &*kept).and_then(|()| kept.placement(&device, &place));
        let made = match placed {
            Ok(made) => made,
            Err(why) => {
                turn.reply(&Placed::Refused(why))?;
                return Ok(());
            }
        };
        let id = self.made(turn, |kept, id, maker| kept.placements.keep(id, maker, made))?;
        turn.reply(&Placed::Made(PlacementId { id }))?;

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
            Release::Instances(instances) => {
                let released = kept.instances.release(instances.id, by);
                if let Owned::Mine(made) = &released {
                    kept.last_drawn(instances.id, made.held.length);
                }
                released.map(drop)
            }
            Release::Uniforms(uniforms) => kept.uniforms.release(uniforms.id, by).map(drop),
            Release::Storage(storage) => kept.storage.release(storage.id, by).map(drop),
            Release::Placement(placement) => kept.placements.release(placement.id, by).map(drop),
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
            for (id, made) in kept.instances.release_all(maker) {
                kept.last_drawn(id, made.held.length);
            }
            kept.uniforms.release_all(maker);
            kept.storage.release_all(maker);
            kept.placements.release_all(maker);
        }

        Ok(())
    }
}

impl Kept {
    /// Notes that the instances `instances` were released holding `length` records: what is placed of them draws
    /// that many from then.
    fn last_drawn(&mut self, instances: u64, length: u32) {
        for placement in self.placements.values_mut() {
            if let Drawing::Records { instances: drawn, length: drawing, .. } = &mut placement.drawing
                && *drawn == instances
            {
                *drawing = Length::Last(length);
            }
        }
    }

    /// The placement `place` asks for, made of what its check found kept, with its bind groups made now.
    fn placement(&mut self, device: &wgpu::Device, place: &Place) -> Result<Placement, PlacementRefused> {
        for bind in &place.bindings {
            if let placement::Resource::Sampler(sampling) = bind.to {
                self.samplers.entry(sampling).or_insert_with(|| sampler(device, sampling));
            }
        }
        let program = self.programs.get(place.program.id).ok_or(PlacementRefused::Program)?;
        let mesh = self.meshes.get(place.mesh.id).ok_or(PlacementRefused::Mesh)?;
        let drawing = match place.instances {
            Drawn::Count(count) => Drawing::Count(count),
            Drawn::Records(instances) => {
                let made = self.instances.get(instances.id).ok_or(PlacementRefused::Instances)?;
                Drawing::Records { instances: instances.id, buffer: made.buffer.clone(), length: Length::Live }
            }
        };
        let mut groups = Vec::with_capacity(program.groups.len());
        for (group, layout) in (0..).zip(&program.groups) {
            let mut entries = Vec::new();
            for bind in place.bindings.iter().filter(|bind| bind.group == group) {
                let missing = PlacementRefused::Missing { group, binding: bind.binding };
                let resource = match bind.to {
                    placement::Resource::Uniforms(uniforms) => {
                        self.uniforms.get(uniforms.id).ok_or(missing)?.buffer.as_entire_binding()
                    }
                    placement::Resource::Storage(storage) => {
                        self.storage.get(storage.id).ok_or(missing)?.buffer.as_entire_binding()
                    }
                    placement::Resource::Texture(texture) => {
                        wgpu::BindingResource::TextureView(&self.textures.get(texture.id).ok_or(missing)?.view)
                    }
                    placement::Resource::Sampler(sampling) => {
                        wgpu::BindingResource::Sampler(self.samplers.get(&sampling).ok_or(missing)?)
                    }
                };
                entries.push(wgpu::BindGroupEntry { binding: bind.binding, resource });
            }
            groups.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout,
                entries: &entries,
            }));
        }

        Ok(Placement {
            program: place.program.id,
            pipeline: program.pipeline.clone(),
            vertices: mesh.vertices.clone(),
            indices: mesh.indices.clone(),
            index_count: mesh.counted.indices,
            drawing,
            groups,
        })
    }

    /// Records every placement into `pass`, each program's together, setting each pipeline once.
    fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let mut placed: Vec<&Placement> = self.placements.values().collect();
        placed.sort_unstable_by_key(|placement| placement.program);
        let mut pipeline = 0;
        for placement in placed {
            let count = match &placement.drawing {
                Drawing::Count(count) => *count,
                Drawing::Records { instances, length: Length::Live, .. } => {
                    self.instances.get(*instances).map_or(0, |made| made.held.length)
                }
                Drawing::Records { length: Length::Last(length), .. } => *length,
            };
            if count == 0 {
                continue;
            }
            // Ids begin at one, so no program's is the zero it begins from.
            if pipeline != placement.program {
                pass.set_pipeline(&placement.pipeline);
                pipeline = placement.program;
            }
            for (group, bound) in (0..).zip(&placement.groups) {
                pass.set_bind_group(group, bound, &[]);
            }
            pass.set_vertex_buffer(0, placement.vertices.slice(..));
            if let Drawing::Records { buffer, .. } = &placement.drawing {
                pass.set_vertex_buffer(1, buffer.slice(..));
            }
            pass.set_index_buffer(placement.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..placement.index_count, 0, 0..count);
        }
    }
}

impl Kept {
    /// Keeps `reading` under a new read, until the chassis says its buffer is mapped, and answers the read with the
    /// ask for the chassis to map it.
    fn reading(&mut self, reading: Reading) -> (u64, Mapping) {
        let read = self.ids.next();
        let mapping = Mapping::new(read, reading.buffer().clone());
        self.reads.insert(read, reading);

        (read, mapping)
    }
}

impl Reading {
    /// The buffer the GPU copies into, which the chassis maps.
    const fn buffer(&self) -> &wgpu::Buffer {
        match self {
            Self::Pixels { buffer, .. } | Self::Stamps { buffer, .. } => buffer,
        }
    }
}

/// The copy of the frame `texture` into a buffer, recorded on `encoder`, to read back once it is mapped: refused
/// for a canvas that cannot be copied from, or of a format no answer is laid out in.
fn copy_frame(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
) -> Result<Reading, ReadRefused> {
    let copyable = texture.usage().contains(wgpu::TextureUsages::COPY_SRC);
    if !copyable {
        return Err(ReadRefused::Uncopied);
    }
    let (format, order) = readback::frame_format(texture.format()).ok_or(ReadRefused::Format)?;

    Ok(copy_texture(device, encoder, texture, format, order))
}

/// The copy of `texture`, of `format`, its channels in `order`, into a buffer the chassis can map, recorded on
/// `encoder`: each row padded to the 256 bytes WebGPU copies rows on.
fn copy_texture(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    format: TextureFormat,
    order: Order,
) -> Reading {
    let copied = Copied::new(texture.width(), texture.height(), format.bytes());
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: copied.bytes(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let layout =
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(copied.row), rows_per_image: Some(copied.height) };
    let size = wgpu::Extent3d { width: copied.width, height: copied.height, depth_or_array_layers: 1 };
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo { buffer: &buffer, layout },
        size,
    );

    Reading::Pixels { buffer, copied, format, order }
}

/// The timestamps a timed frame's pass writes as it begins and ends: where the GPU writes them, where it resolves
/// them to, and the buffer they are copied into for the chassis to map.
struct Stamps {
    set: wgpu::QuerySet,
    resolved: wgpu::Buffer,
    read: wgpu::Buffer,
}

/// The bytes of a pass's two timestamps.
const STAMPS: u64 = 2 * wgpu::QUERY_SIZE as u64;

impl Stamps {
    /// Room for one pass's timestamps.
    fn new(device: &wgpu::Device) -> Self {
        let set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: None,
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        });
        let buffer = |usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: STAMPS,
                usage,
                mapped_at_creation: false,
            })
        };
        let resolved = buffer(wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC);
        let read = buffer(wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);

        Self { set, resolved, read }
    }

    /// Where the pass writes its timestamps: the first as it begins, the second as it ends.
    const fn writes(&self) -> wgpu::RenderPassTimestampWrites<'_> {
        wgpu::RenderPassTimestampWrites {
            query_set: &self.set,
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: Some(1),
        }
    }

    /// Resolves the timestamps and copies them where the chassis can map them, recorded on `encoder` after the pass,
    /// each tick `period` nanoseconds.
    fn resolve(self, encoder: &mut wgpu::CommandEncoder, period: f32) -> Reading {
        encoder.resolve_query_set(&self.set, 0..2, &self.resolved, 0);
        encoder.copy_buffer_to_buffer(&self.resolved, 0, &self.read, 0, STAMPS);

        Reading::Stamps { buffer: self.read, period }
    }
}

/// What a placement's check finds of what is kept.
impl Finds for Kept {
    fn program(&self, id: ProgramId) -> Option<Shape<'_>> {
        let program = self.programs.get(id.id)?;

        Some(Shape { layout: &program.layout, instances: &program.instances, bindings: &program.bindings })
    }

    fn mesh(&self, id: MeshId) -> Option<&VertexLayout> {
        self.meshes.get(id.id).map(|mesh| &mesh.layout)
    }

    fn instances(&self, id: InstancesId) -> Option<&VertexLayout> {
        self.instances.get(id.id).map(|instances| &instances.layout)
    }

    fn uniforms(&self, id: UniformsId) -> Option<u32> {
        self.uniforms.get(id.id).map(|uniforms| uniforms.size)
    }

    fn storage(&self, id: StorageId) -> Option<u32> {
        self.storage.get(id.id).map(|storage| storage.size)
    }

    fn has_texture(&self, id: TextureId) -> bool {
        self.textures.get(id.id).is_some()
    }
}

impl DepthTexture {
    /// A depth texture of `width` by `height`.
    fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Self { view, width, height }
    }

    /// Makes the texture again when the frame is of another size.
    fn fit(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let resized = (width, height) != (self.width, self.height);
        if resized {
            *self = Self::new(device, width, height);
        }
    }
}

/// A sampler that reads as `sampling` says.
fn sampler(device: &wgpu::Device, sampling: Sampling) -> wgpu::Sampler {
    let filter = match sampling.filter {
        Filter::Linear => wgpu::FilterMode::Linear,
        Filter::Nearest => wgpu::FilterMode::Nearest,
    };
    let address = match sampling.wrap {
        Wrap::Clamp => wgpu::AddressMode::ClampToEdge,
        Wrap::Repeat => wgpu::AddressMode::Repeat,
    };

    device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: address,
        address_mode_v: address,
        address_mode_w: address,
        mag_filter: filter,
        min_filter: filter,
        ..Default::default()
    })
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
    let blend = blending(create.state.blend);
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

    Program {
        pipeline,
        layout: create.layout.clone(),
        instances: create.instances.clone(),
        bindings: create.bindings.clone(),
        groups,
    }
}

/// How a program of `blend` lays its colour over what is drawn, as wgpu names it: no blending for one that replaces it.
fn blending(blend: Blend) -> Option<wgpu::BlendState> {
    match blend {
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
        Blend::Multiply => {
            let multiply = wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Dst,
                dst_factor: wgpu::BlendFactor::Zero,
                operation: wgpu::BlendOperation::Add,
            };
            let kept = wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            };
            Some(wgpu::BlendState { color: multiply, alpha: kept })
        }
    }
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
        BindingKind::Storage { bytes } => wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
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
