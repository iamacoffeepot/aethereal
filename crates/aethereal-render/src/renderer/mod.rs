//! The renderer: the root that owns the GPU, makes what actors ask it for, and draws each frame (ADR-0021).

// The tests draw on a device that draws nothing, which only a native build has.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
mod walk;

use crate::Gpu;
use crate::compute::{self, Array, ComputeId, Computed, CreateCompute};
use crate::dispatch::{self, Computes, CreateDispatch, DispatchId, DispatchRefused, Dispatched, Dispatches, Runs, Use};
use crate::draw_list::{DrawList, EntryRefused, ListRefused, Listing, Lists, Placements};
use crate::effect::{self, COVER, COVER_ENTRY, CreateEffect, Effected, MadeEffect, Read};
use crate::frame::{
    self, CreateFrame, DepthUse, Does, EffectReads, Ends, Frame, FrameId, FrameRefused, Framed, Holds, Load,
    PassRefused, ShapeOf, Shaped, Size, Target,
};
use crate::instances::{self, CreateInstances, Instanced, InstancesId, WriteInstances};
use crate::made::{Ids, Made, Makers, Owned};
use crate::mapping::{Mapped, Mapping};
use crate::mesh::{self, Counted, CreateMesh, MeshId, Meshed, VertexLayout};
use crate::pass::PassName;
use crate::placement::{
    self, Bind, Drawn, Filter, Finds, Fits, Join, JoinRefused, Place, Placed, PlacementId, PlacementRefused, Reads,
    Sampling, Wrap,
};
use crate::program::{
    self, Access, Binding, BindingKind, Blend, Coverage, CreateProgram, Cull, Depth, PerInstance, ProgramId, Programmed,
};
use crate::readback::{self, Bytes, Copied, Order, Pixels, ReadBack, ReadRefused, ReadStorage, Readable};
use crate::shape::{ColourFormat, DepthFormat, Shape};
use crate::storage::{self, CreateStorage, StorageId, Stored, WriteStorage};
use crate::surface::{Front, Present, Surfaced, WindowFormat};
use crate::texture::{self, CreateTexture, TextureFormat, TextureId, Textured};
use crate::timing::{self, TimeFrame, Timed, TimingRefused};
use crate::uniforms::{self, CreateUniforms, Uniformed, UniformsId, WriteUniforms};
use aethereal_actor::{
    Actor, Beginning, Carries, ChildOf, Controller, Ended, Ending, Fault, Given, Handle, Handles, Held, Level, On,
    Protocol, Render, Resource, Source, ToSender, Top, Turn,
};
use aethereal_data::{ActorId, Bound, Plain};
use aethereal_window::WindowId;
use std::collections::HashMap;
use std::mem;
use walk::{End, Making, ReadGroup, Step, Walk};

/// The renderer, the one actor that touches the GPU. Actors ask it through [`Rendering`].
#[derive(Bound)]
#[kind(name = "aether.render")]
pub struct Renderer {
    gpu: Handle<Gpu>,
    kept: Handle<Kept>,
    front: Handle<Front>,
    presenting: Handle<Controller<Present>>,
    mapping: Handle<Controller<Mapping>>,
    /// Held so the chassis's word of what it mapped is brought here; nothing opens it.
    mapped: Handle<Source<Mapped>>,
    /// Held so the chassis's word of each window it opened or closed while the program runs is brought here; nothing
    /// opens it.
    surfaced: Handle<Source<Surfaced>>,
    owed: Owed,
}

/// The reads and timings the renderer owes: those of the next frame, and those copied and waiting on their buffer's
/// map, each under the read the buffer is kept by: pixels, storage's bytes, and timings.
#[derive(Bound)]
struct Owed {
    frames: Vec<Held<Pixels>>,
    timings: Vec<Held<Timed>>,
    reading: Vec<(u64, Held<Pixels>)>,
    storing: Vec<(u64, Held<Bytes>)>,
    timing: Vec<(u64, Held<Timed>)>,
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

impl Carries<CreateCompute> for Renderer {
    type Answers = (Computed,);
}

impl Carries<CreateDispatch> for Renderer {
    type Answers = (Dispatched,);
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

impl Carries<ShapeOf> for Renderer {
    type Answers = (Shaped,);
}

impl Carries<CreateEffect> for Renderer {
    type Answers = (Effected,);
}

impl Carries<CreateFrame> for Renderer {
    type Answers = (Framed,);
}

impl Carries<ReadBack> for Renderer {
    type Answers = (Pixels,);
}

impl Carries<ReadStorage> for Renderer {
    type Answers = (Bytes,);
}

impl Carries<TimeFrame> for Renderer {
    type Answers = (Timed,);
}

/// What the renderer is made from: the GPU the chassis opened; the front the chassis hands it each window's frame
/// through, with a slot for each window's surface it keeps ([`WindowSurface::new`](crate::surface::WindowSurface::new)),
/// and the controller it asks the chassis to present each window's frame it drew through; each window it draws to,
/// with the format of the window's surface, in the order the chassis opened them; the controller it asks the chassis
/// to map what it reads back through, and the source the chassis brings word of each through; and the source the
/// chassis brings word of each window it opens or closes while the program runs through (ADR-0019, ADR-0021,
/// ADR-0027).
#[derive(Bound)]
#[kind(name = "aether.render.config")]
pub struct RendererConfig {
    pub gpu: Given<Gpu>,
    pub front: Given<Front>,
    pub presenting: Given<Controller<Present>>,
    pub windows: Vec<WindowFormat>,
    pub mapping: Given<Controller<Mapping>>,
    pub mapped: Given<Source<Mapped>>,
    pub surfaced: Given<Source<Surfaced>>,
}

/// What an actor asks the renderer: to make a mesh, a texture, a program, a compute program, an effect, instances,
/// uniforms, storage, or a frame, to write instances, uniforms, or storage it made, to place what is drawn or dispatch
/// what is computed, to list its placements in a pass in order, the shape of a pass it joins, to release what it made,
/// to read back what it draws or storage, or to time what it draws.
pub struct Rendering;

impl Protocol for Rendering {
    type Kinds = (
        CreateMesh,
        CreateTexture,
        CreateProgram,
        CreateCompute,
        CreateInstances,
        WriteInstances,
        CreateUniforms,
        WriteUniforms,
        CreateStorage,
        WriteStorage,
        Place,
        CreateDispatch,
        DrawList,
        ShapeOf,
        CreateEffect,
        CreateFrame,
        Release,
        ReadBack,
        ReadStorage,
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

impl Carries<CreateCompute> for Rendering {
    type Answers = (Computed,);
}

impl Carries<CreateDispatch> for Rendering {
    type Answers = (Dispatched,);
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

impl Carries<DrawList> for Rendering {
    type Answers = ();
}

impl Carries<ShapeOf> for Rendering {
    type Answers = (Shaped,);
}

impl Carries<CreateEffect> for Rendering {
    type Answers = (Effected,);
}

impl Carries<CreateFrame> for Rendering {
    type Answers = (Framed,);
}

impl Carries<Release> for Rendering {
    type Answers = ();
}

impl Carries<ReadBack> for Rendering {
    type Answers = (Pixels,);
}

impl Carries<ReadStorage> for Rendering {
    type Answers = (Bytes,);
}

impl Carries<TimeFrame> for Rendering {
    type Answers = (Timed,);
}

/// Releases what the renderer made before its maker ends. Only its maker may; what another asks is logged and left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.release")]
pub enum Release {
    Mesh(MeshId),
    Texture(TextureId),
    Program(ProgramId),
    Compute(ComputeId),
    Instances(InstancesId),
    Uniforms(UniformsId),
    Storage(StorageId),
    Placement(PlacementId),
    Dispatch(DispatchId),
    Frame(FrameId),
}

/// What the renderer has made, and who made each, with the samplers its placements, dispatches, and effects share, the
/// lists each actor drew its placements in a pass by, the frames that end on textures in the order they run, `order`,
/// the longest side of a texture and the most workgroups along one dimension the GPU makes and dispatches, and each
/// window it draws to, in the order its chassis opened them.
struct Kept {
    ids: Ids,
    makers: Makers,
    meshes: Made<Mesh>,
    textures: Made<Texture>,
    programs: Made<Program>,
    computes: Made<Compute>,
    instances: Made<Instances>,
    uniforms: Made<Uniforms>,
    storage: Made<Storage>,
    placements: Made<Placement>,
    dispatches: Made<Dispatching>,
    lists: Lists,
    reads: HashMap<u64, Reading>,
    samplers: HashMap<Sampling, wgpu::Sampler>,
    frames: Made<Walk>,
    order: Vec<u64>,
    /// The dispatches not run: each writes a storage another writes in a frame it is in no order with.
    held: Vec<u64>,
    longest: u32,
    most_workgroups: u32,
    windows: Vec<Windowed>,
}

/// A window the renderer draws to: the window, the format of its surface, the renderer's own main frame of it, `own`,
/// and which frame is its main frame, `main`.
struct Windowed {
    window: WindowId,
    format: ColourFormat,
    own: Walk,
    main: Main,
}

impl Windowed {
    /// The window `window`, whose surface is of `format`, drawn by the renderer's own main frame of it until an actor
    /// makes one in its place.
    ///
    /// # Errors
    ///
    /// Why the renderer's own main frame of it was refused.
    fn new(device: &wgpu::Device, window: WindowId, format: ColourFormat) -> Result<Self, FrameRefused> {
        // Made again at the first frame's size, as for each frame of a new size.
        let own = Walk::new(device, CreateFrame::own(window), (End::Window(window), format), Size::PIXEL, effectless)
            .map_err(|(pass, why)| FrameRefused::Pass { pass, why })?;

        Ok(Self { window, format, own, main: Main::Own })
    }
}

/// Which frame is a window's main frame.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Main {
    /// The renderer's own.
    Own,
    /// The frame an actor made, under this id.
    Made(u64),
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
    origin: Origin,
}

/// What a texture holds: the pixels it was made of, or what the frame of this id draws into it at each `Render`.
#[derive(Clone, Copy)]
enum Origin {
    Pixels,
    Frame(u64),
}

/// A buffer the GPU copied into, waiting for the chassis to map it, and how its bytes are read once mapped.
enum Reading {
    /// Pixels, copied so, of `format`, their channels in `order`.
    Pixels { buffer: wgpu::Buffer, copied: Copied, format: TextureFormat, order: Order },
    /// Storage's bytes, copied whole.
    Bytes { buffer: wgpu::Buffer },
    /// A pass's timestamps, each tick `period` nanoseconds.
    Stamps { buffer: wgpu::Buffer, period: f32 },
}

/// A program on the GPU: its pipeline, with what its stages read, what it binds, the layout of each group it binds in,
/// and the shape it draws into.
struct Program {
    pipeline: wgpu::RenderPipeline,
    stages: Stages,
    bindings: Vec<Binding>,
    groups: Vec<wgpu::BindGroupLayout>,
    shape: Shape,
}

/// A compute program on the GPU: its pipeline, what it binds, the layout of each group it binds in, the size of its
/// workgroups, which a dispatch counts the workgroups it runs by, and the storage it binds that ends in an array whose
/// length is its buffer's, which a dispatch for each element counts the elements of (ADR-0026).
struct Compute {
    pipeline: wgpu::ComputePipeline,
    bindings: Vec<Binding>,
    groups: Vec<wgpu::BindGroupLayout>,
    workgroup: [u32; 3],
    arrays: Vec<Array>,
}

/// What a program's stages read.
enum Stages {
    /// It draws placements: vertices laid out as `layout`, what `instances` says once per instance, and the targets of
    /// the pass it joins as `reads` says, bound in its first group when it reads any.
    Draws { layout: VertexLayout, instances: PerInstance, reads: Vec<Read> },
    /// It is an effect, drawn over a whole target, reading targets as `reads` says, bound in its first group, which is
    /// empty when it reads none.
    Effect { reads: Vec<Read> },
}

/// Instances on the GPU: a buffer with room for their capacity, how they stand, and how a record is laid out.
struct Instances {
    buffer: wgpu::Buffer,
    held: instances::Held,
    layout: VertexLayout,
}

/// One thing drawn at each frame, holding all it draws with, so a frame builds nothing and releasing what it was made
/// of frees it only once the placement goes (ADR-0023): its mesh and how many it draws, and each pass it joins.
struct Placement {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    drawing: Drawing,
    joins: Vec<Joined>,
}

/// One pass a placement draws in, with what it draws there with: whether it is drawn loose or by its maker's list, the
/// program's id, which a pass draws loose placements together by, its pipeline and what it was made against, the bind
/// groups made for it, from group 1 when the program reads targets of the pass and from group 0 when it reads none,
/// and the textures and storage they bind, by their ids, with the storage the placement reads its counts from.
struct Joined {
    frame: Frame,
    pass: PassName,
    listing: Listing,
    fits: Fits,
    program: u64,
    pipeline: wgpu::RenderPipeline,
    groups: Vec<wgpu::BindGroup>,
    textures: Vec<u64>,
    storage: Vec<u64>,
}

/// One dispatch run at each frame, holding all it runs with, so a frame builds nothing and releasing what it was made
/// of frees it only once the dispatch goes (ADR-0026): the compute pass it joins, its compute program's id, which a
/// pass runs dispatches together by, and its pipeline, the bind groups made for it, the workgroups it runs, and the
/// textures and storage it binds or reads its workgroups from, by their ids, the storage with how it is used.
struct Dispatching {
    frame: Frame,
    pass: PassName,
    compute: u64,
    pipeline: wgpu::ComputePipeline,
    groups: Vec<wgpu::BindGroup>,
    running: Running,
    textures: Vec<u64>,
    storage: Vec<(u64, Access)>,
}

/// How many workgroups a dispatch runs.
enum Running {
    /// A grid, fixed when it was made.
    Workgroups([u32; 3]),
    /// As many as the three words of `buffer` from the byte `at` hold at the frame, a storage's buffer kept for it.
    Indirect { buffer: wgpu::Buffer, at: u64 },
}

/// How many instances a placement draws.
enum Drawing {
    /// A count, fixed when it was placed.
    Count(u32),
    /// One for each record of the instances `instances`, read from `buffer`.
    Records { instances: u64, buffer: wgpu::Buffer, length: Length },
    /// As the five words of `buffer` from the byte `at` hold at the frame, a storage's buffer kept for it.
    Indirect { buffer: wgpu::Buffer, at: u64 },
}

/// How many records a placement draws from its instances.
#[derive(Clone, Copy)]
enum Length {
    /// As many as the instances hold at the frame: they are still kept, and written.
    Live,
    /// As many as they held when they were released: nothing writes them again.
    Last(u32),
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
        CreateCompute,
        CreateInstances,
        WriteInstances,
        CreateUniforms,
        WriteUniforms,
        CreateStorage,
        WriteStorage,
        Place,
        CreateDispatch,
        DrawList,
        ShapeOf,
        CreateEffect,
        CreateFrame,
        Release,
        ReadBack,
        ReadStorage,
        TimeFrame,
        Mapped,
        Surfaced,
        Ended,
    );
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: RendererConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let mut gpu = turn.take(config.gpu)?;
        let front = turn.take(config.front)?;
        let presenting = turn.take(config.presenting)?;
        let mapping = turn.take(config.mapping)?;
        let mapped = turn.take(config.mapped)?;
        let surfaced = turn.take(config.surfaced)?;
        let opened = turn.open(&mut gpu)?;
        let mut windows = Vec::with_capacity(config.windows.len());
        for WindowFormat { window, format } in config.windows {
            match Windowed::new(opened.device(), window, format) {
                Ok(windowed) => windows.push(windowed),
                Err(why) => return Err(Fault::new(&why)?),
            }
        }
        // Made before it is kept, as keeping it ends the borrow of the GPU opened.
        let kept = Kept::new(opened.device(), windows);
        let kept = turn.keep(kept)?;
        turn.subscribe::<Render>();

        let owed = Owed {
            frames: Vec::new(),
            timings: Vec::new(),
            reading: Vec::new(),
            storing: Vec::new(),
            timing: Vec::new(),
        };

        Ok(Self { gpu, kept, front, presenting, mapping, mapped, surfaced, owed })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Render>> for Renderer {
    type Sends = ();

    fn handle(&mut self, _: Render, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let handed = turn.open(&mut self.front)?.take();
        let gpu = turn.open(&mut self.gpu)?;
        // Held past the opening of what is kept, as a turn opens one resource at a time.
        let (device, queue) = (gpu.device().clone(), gpu.queue().clone());
        let kept = turn.open(&mut self.kept)?;
        // A window the chassis handed no frame for is not drawn this cycle, nor asked to be presented. A cycle it handed
        // none for at all is skipped: nothing is drawn, and the next frame drawn is the one read and timed.
        let frames = kept.ordered(handed);
        let Some((_, first)) = frames.first() else {
            return Ok(());
        };
        let views: Vec<(WindowId, wgpu::TextureView)> = frames
            .iter()
            .map(|(window, frame)| (*window, frame.create_view(&wgpu::TextureViewDescriptor::default())))
            .collect();
        for (window, frame) in &frames {
            kept.fit(&device, *window, Size { width: frame.width(), height: frame.height() });
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let passes = kept.passes(&views);
        let stamps = (!self.owed.timings.is_empty()).then(|| Stamps::new(&device, passes));
        kept.run_all(&mut encoder, &views, stamps.as_ref());
        let copied = (!self.owed.frames.is_empty()).then(|| copy_frame(&device, &mut encoder, first));
        let timed = stamps.map(|stamps| stamps.resolve(&mut encoder, queue.get_timestamp_period()));
        queue.submit([encoder.finish()]);

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
        // Asked once the frames are submitted, so a frame drawn is presented however its answers fare.
        let presenting = turn.open(&mut self.presenting)?;
        for (window, _) in &frames {
            presenting.push(Present::of(*window));
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

impl Handles<ReadStorage> for Renderer {
    type Sends = (ToSender<Bytes>,);

    fn handle(&mut self, read: ReadStorage, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let gpu = turn.open(&mut self.gpu)?;
        let (device, queue) = (gpu.device().clone(), gpu.queue().clone());
        let kept = turn.open(&mut self.kept)?;
        let Some((stored, size)) = kept.storage.get(read.storage.id).map(|kept| (kept.buffer.clone(), kept.size))
        else {
            turn.reply(&Bytes::Refused(ReadRefused::Storage))?;
            return Ok(());
        };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let reading = copy_storage(&device, &mut encoder, &stored, size);
        queue.submit([encoder.finish()]);
        let (read, mapping) = kept.reading(reading);
        let held = turn.hold()?;
        self.owed.storing.push((read, held));
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
            Reading::Bytes { buffer } => {
                let bytes = ready.then(|| mapped_bytes(&buffer, <[u8]>::to_vec)).flatten();
                let answer = match bytes.map(|bytes| turn.put(bytes)) {
                    Some(Ok(bytes)) => Bytes::Read { bytes },
                    Some(Err(_)) => Bytes::Refused(ReadRefused::Unstored),
                    None => Bytes::Refused(ReadRefused::Unmapped),
                };
                for held in owed_by(&mut self.owed.storing, read) {
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

/// What the chassis said of a window it opened or closed while the program runs, brought from the source to itself: a
/// window opened is drawn to from the next `Render` it is handed a frame for, by the renderer's own main frame of it,
/// and a window closed has its main frame dropped, with the targets made for it, so what joined only that frame is
/// drawn nowhere until its maker releases it (ADR-0027).
impl Handles<Surfaced> for Renderer {
    type Sends = ();

    fn handle(&mut self, surfaced: Surfaced, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let brought = turn.sender().id() == turn.me().id();
        if !brought {
            turn.log(
                Level::Warn,
                format_args!("{surfaced:?} was not acted on: only the chassis makes a window's surface"),
            );
            return Ok(());
        }
        let device = turn.open(&mut self.gpu)?.device().clone();
        let kept = turn.open(&mut self.kept)?;
        match surfaced {
            Surfaced::Opened { window, format } => {
                if kept.windowed(window).is_some() {
                    turn.log(Level::Warn, format_args!("window {window:?} was opened again: it is drawn to already"));
                    return Ok(());
                }
                match Windowed::new(&device, window, format) {
                    Ok(windowed) => kept.windows.push(windowed),
                    Err(why) => turn.log(Level::Error, format_args!("window {window:?} is not drawn to: {why:?}")),
                }
            }
            Surfaced::Closed { window } => {
                let closed = kept.closed(window);
                let held = kept.reorder_after_release();
                if !closed {
                    turn.log(Level::Warn, format_args!("window {window:?} was closed, and was not drawn to"));
                }
                log_held(turn, &held);
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
        let texture = Texture { made, view, format, origin: Origin::Pixels };
        let id = self.made(turn, |kept, id, maker| kept.textures.keep(id, maker, texture))?;
        turn.reply(&Textured::Made(TextureId { id }))?;

        Ok(())
    }
}

impl Handles<CreateProgram> for Renderer {
    type Sends = (ToSender<Programmed>,);

    fn handle(&mut self, create: CreateProgram, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        // Copied out of the store, as a mesh's bytes are: the GPU is opened through the turn.
        let bytes = turn.read(&create.wgsl).to_vec();
        let limits = turn.open(&mut self.gpu)?.device().limits();
        let checked = program::source(&bytes).and_then(|wgsl| program::check(&create, wgsl, &limits).map(|()| wgsl));
        let wgsl = match checked {
            Ok(wgsl) => wgsl,
            Err(why) => {
                turn.reply(&Programmed::Refused(why))?;
                return Ok(());
            }
        };

        let gpu = turn.open(&mut self.gpu)?;
        let made = pipeline(gpu, &create, wgsl);
        let id = self.made(turn, |kept, id, maker| kept.programs.keep(id, maker, made))?;
        turn.reply(&Programmed::Made(ProgramId { id }))?;

        Ok(())
    }
}

impl Handles<CreateCompute> for Renderer {
    type Sends = (ToSender<Computed>,);

    fn handle(&mut self, create: CreateCompute, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let bytes = turn.read(&create.wgsl).to_vec();
        let limits = turn.open(&mut self.gpu)?.device().limits();
        let checked = program::source(&bytes).and_then(|wgsl| Ok((wgsl, compute::check(&create, wgsl, &limits)?)));
        let (wgsl, checked) = match checked {
            Ok(checked) => checked,
            Err(why) => {
                turn.reply(&Computed::Refused(why))?;
                return Ok(());
            }
        };

        let gpu = turn.open(&mut self.gpu)?;
        let made = compute_pipeline(gpu.device(), &create, wgsl, checked);
        let id = self.made(turn, |kept, id, maker| kept.computes.keep(id, maker, made))?;
        turn.reply(&Computed::Made(ComputeId { id }))?;

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
        // Copied from as it is read back, and read as a draw's or a dispatch's arguments by any that asks to.
        let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_SRC;
        let buffer = upload(gpu, &bytes, usage);
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
            let fits = storage::fits(storage.size, write.first, bytes.len() as u64);
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
        let id = self.made(turn, |kept, id, maker| {
            kept.placements.keep(id, maker, made);
            kept.reorder();
        })?;
        turn.reply(&Placed::Made(PlacementId { id }))?;

        Ok(())
    }
}

impl Handles<CreateDispatch> for Renderer {
    type Sends = (ToSender<Dispatched>,);

    fn handle(&mut self, create: CreateDispatch, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let by = turn.sender().id();
        let device = turn.open(&mut self.gpu)?.device().clone();
        let kept = turn.open(&mut self.kept)?;
        let made = dispatch::check(&create, by, &*kept).and_then(|runs| kept.dispatch(&device, &create, runs));
        let made = match made {
            Ok(made) => made,
            Err(why) => {
                turn.reply(&Dispatched::Refused(why))?;
                return Ok(());
            }
        };
        let id = self.made(turn, |kept, id, maker| {
            kept.dispatches.keep(id, maker, made);
            kept.reorder();
        })?;
        turn.reply(&Dispatched::Made(DispatchId { id }))?;

        Ok(())
    }
}

/// A list of the sender's own placements in a pass, checked whole and kept in place of its last for the pass, or
/// refused and logged, the last standing (ADR-0031). The sender is watched from the first list it sends, as from the
/// first thing it made, so its lists go when it ends.
impl Handles<DrawList> for Renderer {
    type Sends = ();

    fn handle(&mut self, list: DrawList, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let listed = self.kept_for(turn, |kept, by| kept.listed(&list, by))?;
        if let Err(why) = listed {
            let DrawList { frame, pass, .. } = list;
            turn.log(Level::Warn, format_args!("the list for {pass:?} of {frame:?} was refused: {why:?}"));
        }

        Ok(())
    }
}

impl Handles<ShapeOf> for Renderer {
    type Sends = (ToSender<Shaped>,);

    fn handle(&mut self, of: ShapeOf, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let kept = turn.open(&mut self.kept)?;
        let shaped = match kept.pass(of.frame, of.pass) {
            Ok(fits) => Shaped::Shape(fits.shape),
            Err(why) => Shaped::Refused(why),
        };
        turn.reply(&shaped)?;

        Ok(())
    }
}

impl Handles<CreateEffect> for Renderer {
    type Sends = (ToSender<Effected>,);

    fn handle(&mut self, create: CreateEffect, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let bytes = turn.read(&create.wgsl).to_vec();
        let limits = turn.open(&mut self.gpu)?.device().limits();
        let checked = program::source(&bytes).and_then(|wgsl| effect::check(&create, wgsl, &limits).map(|()| wgsl));
        let wgsl = match checked {
            Ok(wgsl) => wgsl,
            Err(why) => {
                turn.reply(&Effected::Refused(why))?;
                return Ok(());
            }
        };

        let gpu = turn.open(&mut self.gpu)?;
        let made = effect_pipeline(gpu.device(), &create, wgsl);
        let id = self.made(turn, |kept, id, maker| kept.programs.keep(id, maker, made))?;
        turn.reply(&Effected::Made(MadeEffect { program: ProgramId { id }, reads: create.reads }))?;

        Ok(())
    }
}

impl Handles<CreateFrame> for Renderer {
    type Sends = (ToSender<Framed>,);

    fn handle(&mut self, create: CreateFrame, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let by = turn.sender().id();
        let device = turn.open(&mut self.gpu)?.device().clone();
        let kept = turn.open(&mut self.kept)?;
        let checked = kept.may_make(create.ends, by).and_then(|()| frame::check(&create, &*kept));
        if let Err(why) = checked {
            turn.reply(&Framed::Refused(why))?;
            return Ok(());
        }

        let framed = self.kept_for(turn, |kept, maker| kept.frame(&device, create, maker))?;
        let kept = turn.open(&mut self.kept)?;
        let (undrawn, unrun) = (kept.undrawn(), kept.unrun());
        for (placement, window, pass) in undrawn {
            turn.log(
                Level::Warn,
                format_args!(
                    "placement {placement} is not drawn: the main frame of window {window:?} has no pass {pass:?} of \
                     its shape"
                ),
            );
        }
        for (dispatch, window, pass) in unrun {
            turn.log(
                Level::Warn,
                format_args!(
                    "dispatch {dispatch} is not run: the main frame of window {window:?} has no compute pass {pass:?}"
                ),
            );
        }
        turn.reply(&framed.unwrap_or_else(Framed::Refused))?;

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
            Release::Compute(compute) => kept.computes.release(compute.id, by).map(drop),
            Release::Instances(instances) => {
                let released = kept.instances.release(instances.id, by);
                if let Owned::Mine(made) = &released {
                    kept.last_drawn(instances.id, made.held.length);
                }
                released.map(drop)
            }
            Release::Uniforms(uniforms) => kept.uniforms.release(uniforms.id, by).map(drop),
            Release::Storage(storage) => kept.storage.release(storage.id, by).map(drop),
            Release::Placement(placement) => kept.release_placement(placement.id, by),
            Release::Dispatch(dispatch) => kept.dispatches.release(dispatch.id, by).map(drop),
            Release::Frame(frame) => {
                let released = kept.frames.release(frame.id, by);
                if let Owned::Mine(walk) = &released {
                    kept.unframed([(frame.id, walk.texture())]);
                }
                released.map(drop)
            }
        };
        let held = kept.reorder_after_release();
        log_held(turn, &held);
        match released {
            Owned::Mine(()) => {}
            Owned::NotOwner => turn.log(Level::Warn, format_args!("{release:?} was not released: another made it")),
            Owned::Unknown => turn.log(Level::Warn, format_args!("{release:?} was not released: no such thing")),
        }

        Ok(())
    }
}

/// Logs each dispatch of `held`, which is not run until its frame is ordered again with the others that write what it
/// writes.
fn log_held<S>(turn: &mut Turn<'_, Renderer, S>, held: &[u64]) {
    for dispatch in held {
        turn.log(
            Level::Warn,
            format_args!("dispatch {dispatch} is not run: a frame in no order with its own writes what it writes"),
        );
    }
}

impl Handles<Ended> for Renderer {
    type Sends = ();

    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let maker = ended.actor().id();
        let kept = turn.open(&mut self.kept)?;
        let watched = kept.makers.ended(maker);
        if watched {
            let held = kept.ended(maker);
            log_held(turn, &held);
        }

        Ok(())
    }
}

impl Kept {
    /// Nothing made yet, on `device`, for the windows `windows`, each drawn by the renderer's own main frame of it.
    fn new(device: &wgpu::Device, windows: Vec<Windowed>) -> Self {
        Self {
            ids: Ids::default(),
            makers: Makers::default(),
            meshes: Made::default(),
            textures: Made::default(),
            programs: Made::default(),
            computes: Made::default(),
            instances: Made::default(),
            uniforms: Made::default(),
            storage: Made::default(),
            placements: Made::default(),
            dispatches: Made::default(),
            lists: Lists::default(),
            reads: HashMap::new(),
            samplers: HashMap::new(),
            frames: Made::default(),
            order: Vec::new(),
            held: Vec::new(),
            longest: device.limits().max_texture_dimension_2d,
            most_workgroups: device.limits().max_compute_workgroups_per_dimension,
            windows,
        }
    }

    /// Releases the placement `placement`, when `by` made it, taking it out of `by`'s lists in this turn (ADR-0031).
    fn release_placement(&mut self, placement: u64, by: ActorId) -> Owned<()> {
        let released = self.placements.release(placement, by);
        if let Owned::Mine(made) = &released {
            self.lists.released(placement, by, made.listed());
        }

        released.map(drop)
    }

    /// Releases everything `maker`, which ended, made, with its lists, and answers each dispatch that ran before and
    /// is held now.
    fn ended(&mut self, maker: ActorId) -> Vec<u64> {
        self.meshes.release_all(maker);
        self.textures.release_all(maker);
        self.programs.release_all(maker);
        self.computes.release_all(maker);
        for (id, made) in self.instances.release_all(maker) {
            self.last_drawn(id, made.held.length);
        }
        self.uniforms.release_all(maker);
        self.storage.release_all(maker);
        self.placements.release_all(maker);
        self.lists.ended(maker);
        self.dispatches.release_all(maker);
        let frames: Vec<(u64, Option<u64>)> =
            self.frames.release_all(maker).into_iter().map(|(id, walk)| (id, walk.texture())).collect();
        self.unframed(frames);

        self.reorder_after_release()
    }

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

    /// The placement `place` asks for, made of what its check found kept, with each join's bind groups made now.
    fn placement(&mut self, device: &wgpu::Device, place: &Place) -> Result<Placement, PlacementRefused> {
        self.sampled(device, place.joins.iter().flat_map(|join| &join.bindings));
        let mesh = self.meshes.get(place.mesh.id).ok_or(PlacementRefused::Mesh)?;
        let drawing = match place.instances {
            Drawn::Count(count) => Drawing::Count(count),
            Drawn::Records(instances) => {
                let made = self.instances.get(instances.id).ok_or(PlacementRefused::Instances)?;
                Drawing::Records { instances: instances.id, buffer: made.buffer.clone(), length: Length::Live }
            }
            Drawn::Indirect { storage, at } => {
                let made = self.storage.get(storage.id).ok_or(PlacementRefused::Storage)?;
                Drawing::Indirect { buffer: made.buffer.clone(), at: u64::from(at) }
            }
        };
        let mut joins = Vec::with_capacity(place.joins.len());
        for (join, at) in place.joins.iter().zip(0..) {
            let joined =
                self.joined(device, join, place.instances).map_err(|why| PlacementRefused::Join { join: at, why })?;
            joins.push(joined);
        }

        Ok(Placement {
            vertices: mesh.vertices.clone(),
            indices: mesh.indices.clone(),
            index_count: mesh.counted.indices,
            drawing,
            joins,
        })
    }

    /// The pass `join` draws in, with its program's pipeline and the bind groups of what it binds, made now, for a
    /// placement drawn as `drawn` says, whose storage it reads its counts from, if any, is read in the pass.
    fn joined(&self, device: &wgpu::Device, join: &Join, drawn: Drawn) -> Result<Joined, JoinRefused> {
        let program = self.programs.get(join.program.id).ok_or(JoinRefused::Program)?;
        let Stages::Draws { reads, .. } = &program.stages else {
            return Err(JoinRefused::Program);
        };
        // A program that reads targets binds them in its first group, which the pass binds.
        let first = u32::from(!reads.is_empty());

        let listing = match join.order {
            placement::Order::Loose => Listing::Loose,
            placement::Order::Listed => Listing::Listed { stamp: 0 },
        };

        Ok(Joined {
            frame: join.frame,
            pass: join.pass,
            listing,
            fits: Fits { shape: program.shape, reads: reads.clone() },
            program: join.program.id,
            pipeline: program.pipeline.clone(),
            groups: self.bind_groups(device, &program.groups[first as usize..], &join.bindings, first)?,
            textures: textures(&join.bindings),
            storage: join
                .bindings
                .iter()
                .filter_map(|bind| match bind.to {
                    placement::Resource::Storage(storage) => Some(storage.id),
                    placement::Resource::Uniforms(_)
                    | placement::Resource::Texture(_)
                    | placement::Resource::Sampler(_) => None,
                })
                .chain(drawn.arguments().map(|storage| storage.id))
                .collect(),
        })
    }

    /// The dispatch `create` asks for, made of what its check found kept, running as `runs` says, with its bind groups
    /// made now.
    fn dispatch(
        &mut self,
        device: &wgpu::Device,
        create: &CreateDispatch,
        runs: Runs,
    ) -> Result<Dispatching, DispatchRefused> {
        let join = &create.join;
        self.sampled(device, &join.bindings);
        let compute = self.computes.get(join.compute.id).ok_or(DispatchRefused::Compute)?;
        let groups = self
            .bind_groups(device, &compute.groups, &join.bindings, 0)
            .map_err(|why| DispatchRefused::Bindings { why })?;
        let storage = join
            .bindings
            .iter()
            .filter_map(|bind| {
                let placement::Resource::Storage(storage) = bind.to else {
                    return None;
                };
                let access = match program::listed(&compute.bindings, bind.group, bind.binding)? {
                    BindingKind::Storage { access, .. } => access,
                    BindingKind::Uniform { .. } | BindingKind::Texture | BindingKind::Sampler => return None,
                };
                Some((storage.id, access))
            })
            .chain(create.size.arguments().map(|storage| (storage.id, Access::Read)))
            .collect();
        let running = match runs {
            Runs::Workgroups(workgroups) => Running::Workgroups(workgroups),
            Runs::Indirect { storage, at } => {
                let made = self.storage.get(storage.id).ok_or(DispatchRefused::Storage)?;
                Running::Indirect { buffer: made.buffer.clone(), at: u64::from(at) }
            }
        };

        Ok(Dispatching {
            frame: join.frame,
            pass: join.pass,
            compute: join.compute.id,
            pipeline: compute.pipeline.clone(),
            groups,
            running,
            textures: textures(&join.bindings),
            storage,
        })
    }

    /// Makes the sampler of each way `bindings` samples that none has made yet: one sampler for each way, shared by
    /// every placement, dispatch, and effect that samples so.
    fn sampled<'a>(&mut self, device: &wgpu::Device, bindings: impl IntoIterator<Item = &'a Bind>) {
        for bind in bindings {
            if let placement::Resource::Sampler(sampling) = bind.to {
                self.samplers.entry(sampling).or_insert_with(|| sampler(device, sampling));
            }
        }
    }

    /// A bind group for each of `layouts`, the first of the group `first`, binding what `bindings` names in it.
    fn bind_groups(
        &self,
        device: &wgpu::Device,
        layouts: &[wgpu::BindGroupLayout],
        bindings: &[Bind],
        first: u32,
    ) -> Result<Vec<wgpu::BindGroup>, JoinRefused> {
        let mut groups = Vec::with_capacity(layouts.len());
        for (group, layout) in (first..).zip(layouts) {
            let mut entries = Vec::new();
            for bind in bindings.iter().filter(|bind| bind.group == group) {
                let missing = JoinRefused::Missing { group, binding: bind.binding };
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

        Ok(groups)
    }

    /// Records the frame `walk`, joined as `frame`, on `encoder`, a frame that ends on a window drawing into that
    /// window's frame among `windows`: each of its passes begun in order, and what it does recorded in it, each pass
    /// writing its timestamps to `stamps` from the pass `first` of the `Render`, when the frames are timed. A frame
    /// that ends on a window with none among them is not recorded.
    fn run(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        (frame, walk): (Frame, &Walk),
        windows: &[(WindowId, wgpu::TextureView)],
        (stamps, first): (Option<&Stamps>, u32),
    ) {
        let Some(end) = walk.end(windows) else {
            return;
        };
        for (at, (pass, step)) in (first..).zip(walk.steps()) {
            let (into, load, depth) = match step {
                Step::Draw { into, depth, load, .. } => (*into, *load, *depth),
                Step::Effect { effect, .. } => (effect.into, effect.load, DepthUse::Off),
                Step::Compute => {
                    let mut recording = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: None,
                        timestamp_writes: stamps.map(|stamps| stamps.compute_writes(at)),
                    });
                    self.compute(&mut recording, frame, pass);
                    continue;
                }
            };
            // A target of four samples is drawn into as its four, kept for a later pass that draws over them, and
            // resolved as the pass ends into the one a later pass reads.
            let (view, resolve_target) = match into {
                Target::End => (end, None),
                Target::Texture(target) => (walk.target(target).drawn(), walk.target(target).resolved()),
            };
            let load = match load {
                Load::Clear => wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                Load::Keep => wgpu::LoadOp::Load,
            };
            let colour = wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target,
                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
            };
            let depth_ops = |load| Some(wgpu::Operations { load, store: wgpu::StoreOp::Store });
            let depth = match depth {
                DepthUse::Off => None,
                DepthUse::Write(target) => Some(wgpu::RenderPassDepthStencilAttachment {
                    view: walk.target(target).drawn(),
                    depth_ops: depth_ops(wgpu::LoadOp::Clear(1.0)),
                    stencil_ops: None,
                }),
                DepthUse::Test(target) => Some(wgpu::RenderPassDepthStencilAttachment {
                    view: walk.target(target).drawn(),
                    depth_ops: depth_ops(wgpu::LoadOp::Load),
                    stencil_ops: None,
                }),
            };
            let mut recording = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(colour)],
                depth_stencil_attachment: depth,
                timestamp_writes: stamps.map(|stamps| stamps.writes(at)),
                ..Default::default()
            });
            match step {
                Step::Draw { fits, read, .. } => self.draw(&mut recording, (frame, pass), fits, read),
                Step::Effect { effect, .. } => {
                    recording.set_pipeline(&effect.pipeline);
                    recording.set_bind_group(0, &effect.read.group, &[]);
                    for (group, bound) in (1..).zip(&effect.groups) {
                        recording.set_bind_group(group, bound, &[]);
                    }
                    recording.draw(0..3, 0..1);
                }
                Step::Compute => {}
            }
        }
    }

    /// Drops what the renderer draws to `window` with, which has closed: its own main frame of it, and the frame an
    /// actor made in its place, with what each made for it, and the lists for it, as no window's id is given again.
    /// Answers whether it drew to the window.
    fn closed(&mut self, window: WindowId) -> bool {
        let Some(at) = self.windows.iter().position(|windowed| windowed.window == window) else {
            return false;
        };
        let closed = self.windows.remove(at);
        if let Main::Made(id) = closed.main {
            self.frames.forget(id);
        }
        self.lists.unframed(Frame::Main(window));

        true
    }

    /// The window `window`, as the renderer draws to it: none when it does not.
    fn windowed(&self, window: WindowId) -> Option<&Windowed> {
        self.windows.iter().find(|windowed| windowed.window == window)
    }

    /// The main frame of the window `windowed`.
    fn main_of<'a>(&'a self, windowed: &'a Windowed) -> &'a Walk {
        match windowed.main {
            Main::Own => &windowed.own,
            Main::Made(id) => self.frames.get(id).unwrap_or(&windowed.own),
        }
    }

    /// The main frame of `window`: none when the renderer does not draw to it.
    fn main(&self, window: WindowId) -> Option<&Walk> {
        self.windowed(window).map(|windowed| self.main_of(windowed))
    }

    /// Fits the main frame of `window` to its window, drawn at `size` this `Render`: none is fitted when the renderer
    /// does not draw to it.
    fn fit(&mut self, device: &wgpu::Device, window: WindowId, size: Size) {
        let Some(windowed) = self.windows.iter_mut().find(|windowed| windowed.window == window) else {
            return;
        };
        let main = match windowed.main {
            Main::Own => &mut windowed.own,
            Main::Made(id) => self.frames.get_mut(id).unwrap_or(&mut windowed.own),
        };
        main.fit(device, size);
    }

    /// The frames `handed`, each beside its window, in the order of the windows the renderer draws to: a frame handed
    /// for a window it does not draw to is dropped.
    fn ordered(&self, mut handed: Vec<(WindowId, wgpu::Texture)>) -> Vec<(WindowId, wgpu::Texture)> {
        let mut ordered = Vec::with_capacity(handed.len());
        for windowed in &self.windows {
            let at = handed.iter().position(|(window, _)| *window == windowed.window);
            if let Some(at) = at {
                ordered.push(handed.swap_remove(at));
            }
        }

        ordered
    }

    /// Notes that the frames `released` are gone, each with the texture it ends on, if any: a window's main frame is
    /// the renderer's own again when it was one of them, with the lists for it, and the id of each texture is taken
    /// back, as the frames and placements that read it keep it as it was last drawn, with the lists for the frame that
    /// ended on it dropped (ADR-0025, ADR-0031).
    fn unframed(&mut self, released: impl IntoIterator<Item = (u64, Option<u64>)>) {
        for (id, texture) in released {
            for windowed in &mut self.windows {
                if windowed.main == Main::Made(id) {
                    windowed.main = Main::Own;
                }
            }
            if let Some(texture) = texture {
                self.textures.forget(texture);
                self.lists.unframed(Frame::Texture(FrameId { id }));
            }
        }
    }

    /// Checks `list`, sent by `by`, whole, and keeps it in place of `by`'s last list for its pass: refused when its
    /// frame is not kept, or as [`Lists::check`] refuses it.
    fn listed(&mut self, list: &DrawList, by: ActorId) -> Result<(), ListRefused> {
        let framed = match list.frame {
            Frame::Main(window) => self.windowed(window).is_some(),
            Frame::Texture(frame) => self.frames.get(frame.id).is_some_and(|walk| walk.texture().is_some()),
        };
        if !framed {
            return Err(ListRefused::Frame);
        }

        self.lists.check(list, by, &mut self.placements)
    }

    /// Who owns the main frame of `window`: the maker of the frame that is it, none while it is the renderer's own or
    /// the renderer does not draw to the window.
    fn owner(&self, window: WindowId) -> Option<ActorId> {
        let Main::Made(id) = self.windowed(window)?.main else {
            return None;
        };

        self.frames.maker(id)
    }

    /// Checks that `by` may make a frame that ends as `ends` says: only the owner of a window's main frame replaces
    /// it.
    fn may_make(&self, ends: Ends, by: ActorId) -> Result<(), FrameRefused> {
        match ends {
            Ends::Window(window) => frame::may_replace(self.owner(window), by),
            Ends::Texture { .. } => Ok(()),
        }
    }

    /// Makes the frame `create`, which its check let through, for `maker`: a window's main frame in place of the one
    /// before, which is dropped, or a frame that ends on a texture of its own, kept as an ordinary texture.
    fn frame(&mut self, device: &wgpu::Device, create: CreateFrame, maker: ActorId) -> Result<Framed, FrameRefused> {
        let id = self.ids.next();
        let (end, format, size) = match create.ends {
            Ends::Window(window) => {
                let format = self.windowed(window).ok_or(FrameRefused::Window)?.format;
                (End::Window(window), format, Size::PIXEL)
            }
            Ends::Texture { size: Size { width, height }, format } => {
                let texture = self.ids.next();
                let made = device.create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: format.wgpu(),
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                });
                let view = made.create_view(&wgpu::TextureViewDescriptor::default());
                let origin = Origin::Frame(id);
                self.textures.keep(texture, maker, Texture { made, view: view.clone(), format, origin });
                (End::Texture { texture, view }, format.colour(), Size { width, height })
            }
        };
        let texture = match end {
            End::Window(_) => None,
            End::Texture { texture, .. } => Some(texture),
        };
        self.sampled(device, create.passes.iter().flat_map(|pass| pass.does.bindings()));
        let walked = Walk::new(device, create, (end, format), size, |making| self.step(device, making));
        let walk = match walked {
            Ok(walk) => walk,
            Err((pass, why)) => {
                if let Some(texture) = texture {
                    self.textures.forget(texture);
                }
                return Err(FrameRefused::Pass { pass, why });
            }
        };
        let made = walk.frame(id);
        self.frames.keep(id, maker, walk);

        let framed = if let Some(texture) = texture {
            Framed::Texture { frame: FrameId { id }, texture: TextureId { id: texture } }
        } else {
            // The frame it replaces is the window's no longer, and is dropped: a window has one main frame.
            let shown = self.windows.iter_mut().find(|windowed| made == Frame::Main(windowed.window));
            if let Some(windowed) = shown {
                let replaced = mem::replace(&mut windowed.main, Main::Made(id));
                if let Main::Made(replaced) = replaced {
                    self.frames.forget(replaced);
                }
            }
            Framed::Made(FrameId { id })
        };
        self.reorder();

        Ok(framed)
    }

    /// Orders the frames that end on textures to run each after the frames it reads.
    fn reorder(&mut self) {
        let frames: Vec<u64> =
            self.frames.iter().filter(|(_, walk)| walk.texture().is_some()).map(|(id, _)| id).collect();
        let reads = self.reads();
        self.order = frame::ordered(&frames, &reads);
        let writers: Vec<(u64, Frame, u64)> = self
            .dispatches
            .iter()
            .flat_map(|(id, dispatch)| {
                let written = dispatch.storage.iter().filter(|(_, access)| *access == Access::Write);
                written.map(move |(storage, _)| (id, dispatch.frame, *storage))
            })
            .collect();
        self.held = dispatch::unordered(&writers, &reads);
    }

    /// Orders the frames again, as [`reorder`](Self::reorder) does, after something that ordered them went, and
    /// answers each dispatch that ran before and is held now.
    fn reorder_after_release(&mut self) -> Vec<u64> {
        let before = self.held.clone();
        self.reorder();

        self.held.iter().filter(|id| !before.contains(id)).copied().collect()
    }

    /// The main frame of each window with a frame among `windows`, beside the window, in the order of the windows
    /// the renderer draws to.
    fn shown<'a>(
        &'a self,
        windows: &'a [(WindowId, wgpu::TextureView)],
    ) -> impl Iterator<Item = (WindowId, &'a Walk)> + 'a {
        self.windows
            .iter()
            .filter(|windowed| windows.iter().any(|(window, _)| *window == windowed.window))
            .map(|windowed| (windowed.window, self.main_of(windowed)))
    }

    /// How many passes a `Render` runs that draws into `windows`: every frame's that ends on a texture, and the main
    /// frame's of each window with a frame among them.
    fn passes(&self, windows: &[(WindowId, wgpu::TextureView)]) -> u32 {
        let textures: u32 = self.order.iter().filter_map(|id| self.frames.get(*id)).map(Walk::passes).sum();
        let mains: u32 = self.shown(windows).map(|(_, main)| main.passes()).sum();

        textures + mains
    }

    /// Records every frame on `encoder`: those that end on textures in their order, then the main frame of each window
    /// with a frame among `windows`, in the order of the windows, drawing into that frame, each pass writing its
    /// timestamps to `stamps` in the order they run when the frames are timed (ADR-0025).
    fn run_all(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        windows: &[(WindowId, wgpu::TextureView)],
        stamps: Option<&Stamps>,
    ) {
        let mut first = 0;
        for id in &self.order {
            if let Some(walk) = self.frames.get(*id) {
                self.run(encoder, (Frame::Texture(FrameId { id: *id }), walk), windows, (stamps, first));
                first += walk.passes();
            }
        }
        for (window, main) in self.shown(windows) {
            self.run(encoder, (Frame::Main(window), main), windows, (stamps, first));
            first += main.passes();
        }
    }

    /// The effect pass the frame `create` does at `making`, made: the effect's pipeline and bind groups, made of what
    /// the frame's check found kept.
    fn step(&self, device: &wgpu::Device, making: Making<'_>) -> Result<walk::Effected, PassRefused> {
        let Does::Effect { program, reads, into, bindings } = making.does else {
            return Err(PassRefused::Effect);
        };
        let made = self.programs.get(program.id).ok_or(PassRefused::Effect)?;
        let reads: Vec<u32> = reads
            .iter()
            .map(|read| match read {
                Target::Texture(at) => Ok(*at),
                Target::End => Err(PassRefused::End),
            })
            .collect::<Result<_, _>>()?;
        let layout = made.groups.first().ok_or(PassRefused::Effect)?.clone();
        let read = ReadGroup::new(device, layout, reads, making.targets);
        let groups = self
            .bind_groups(device, made.groups.get(1..).unwrap_or_default(), bindings, 1)
            .map_err(|why| PassRefused::Bindings { why })?;
        let load = if making.written {
            Load::Keep
        } else {
            Load::Clear
        };

        Ok(walk::Effected { pipeline: made.pipeline.clone(), read, groups, into: *into, load })
    }

    /// Each placement's joins of a window's main frame that it does not draw: those whose pass it has not, or has in
    /// another shape or reading other targets, each as its placement, the window, and the pass it joined.
    fn undrawn(&self) -> Vec<(u64, WindowId, PassName)> {
        self.placements
            .iter()
            .flat_map(|(id, placement)| placement.joins.iter().map(move |join| (id, join)))
            .filter_map(|(id, join)| {
                let Frame::Main(window) = join.frame else {
                    return None;
                };
                let drawn = self.main(window)?.fits(join.pass).is_some_and(|fits| fits == join.fits);
                (!drawn).then_some((id, window, join.pass))
            })
            .collect()
    }

    /// Each dispatch that joins a window's main frame and is not run: one whose compute pass it has not, each as the
    /// dispatch, the window, and the pass it joined.
    fn unrun(&self) -> Vec<(u64, WindowId, PassName)> {
        self.dispatches
            .iter()
            .filter_map(|(id, dispatch)| {
                let Frame::Main(window) = dispatch.frame else {
                    return None;
                };
                let runs = self.main(window)?.computes(dispatch.pass);
                (!runs).then_some((id, window, dispatch.pass))
            })
            .collect()
    }

    /// Records into `recording` every dispatch that joins the compute pass `pass` of `frame`, each compute program's
    /// together, setting each pipeline once. Nothing orders them among themselves: the check refuses two in one pass
    /// that one writes what the other uses.
    fn compute(&self, recording: &mut wgpu::ComputePass<'_>, frame: Frame, pass: PassName) {
        let mut joined: Vec<&Dispatching> = self
            .dispatches
            .iter()
            .filter(|(id, dispatch)| {
                let joins = dispatch.runs_in(frame, pass);
                let held = self.held.contains(id);
                joins && !held
            })
            .map(|(_, dispatch)| dispatch)
            .collect();
        joined.sort_unstable_by_key(|dispatch| dispatch.compute);
        let mut pipeline = 0;
        for dispatch in joined {
            // Ids begin at one, so no compute program's is the zero it begins from.
            if pipeline != dispatch.compute {
                recording.set_pipeline(&dispatch.pipeline);
                pipeline = dispatch.compute;
            }
            for (group, bound) in (0..).zip(&dispatch.groups) {
                recording.set_bind_group(group, bound, &[]);
            }
            match &dispatch.running {
                Running::Workgroups([x, y, z]) => recording.dispatch_workgroups(*x, *y, *z),
                Running::Indirect { buffer, at } => recording.dispatch_workgroups_indirect(buffer, *at),
            }
        }
    }

    /// Records into `recording` every placement drawn in the pass `pass` of `frame`, whose programs are made against
    /// `fits`, in the order [`drawn`](Self::drawn) answers, setting a pipeline only where the placement drawn before
    /// used another program, with the targets the pass reads bound at group 0 through `read` when it reads any.
    fn draw(
        &self,
        recording: &mut wgpu::RenderPass<'_>,
        (frame, pass): (Frame, PassName),
        fits: &Fits,
        read: &ReadGroup,
    ) {
        // The targets it reads are bound once, for every program that joins it, which lists nothing in group 0.
        let reads = !fits.reads.is_empty();
        if reads {
            recording.set_bind_group(0, &read.group, &[]);
        }
        let first = u32::from(reads);
        let mut pipeline = 0;
        for (_, placement, join) in self.drawn((frame, pass), fits) {
            let count = match &placement.drawing {
                Drawing::Count(count) => Count::Given(*count),
                Drawing::Records { instances, length: Length::Live, .. } => {
                    Count::Given(self.instances.get(*instances).map_or(0, |made| made.held.length))
                }
                Drawing::Records { length: Length::Last(length), .. } => Count::Given(*length),
                Drawing::Indirect { buffer, at } => Count::Held { buffer, at: *at },
            };
            if matches!(count, Count::Given(0)) {
                continue;
            }
            // Ids begin at one, so no program's is the zero it begins from.
            if pipeline != join.program {
                recording.set_pipeline(&join.pipeline);
                pipeline = join.program;
            }
            for (group, bound) in (first..).zip(&join.groups) {
                recording.set_bind_group(group, bound, &[]);
            }
            recording.set_vertex_buffer(0, placement.vertices.slice(..));
            if let Drawing::Records { buffer, .. } = &placement.drawing {
                recording.set_vertex_buffer(1, buffer.slice(..));
            }
            recording.set_index_buffer(placement.indices.slice(..), wgpu::IndexFormat::Uint32);
            match count {
                Count::Given(count) => recording.draw_indexed(0..placement.index_count, 0, 0..count),
                Count::Held { buffer, at } => recording.draw_indexed_indirect(buffer, at),
            }
        }
    }

    /// Each placement drawn in the pass `pass` of `frame`, whose programs are made against `fits`, under its id, with
    /// its join of the pass, in the order it is drawn: those joined loose, each program's together, then each actor's
    /// list back to back, in its order (ADR-0031). A join of another shape, or reading other targets, is not drawn,
    /// and neither is one joined listed that no list holds.
    fn drawn(&self, (frame, pass): (Frame, PassName), fits: &Fits) -> Vec<(u64, &Placement, &Joined)> {
        let mut drawn: Vec<(u64, &Placement, &Joined)> = self
            .placements
            .iter()
            .flat_map(|(id, placement)| placement.joins.iter().map(move |join| (id, placement, join)))
            .filter(|(_, _, join)| {
                let draws = join.draws_in(frame, pass, fits);
                let unlisted = join.listing == Listing::Loose;
                draws && unlisted
            })
            .collect();
        drawn.sort_unstable_by_key(|(_, _, join)| join.program);
        let listed = self.lists.of(frame, pass).flatten().filter_map(|entry| {
            // Neither misses: a release takes its placement out of its maker's lists in the release's turn.
            let placement = self.placements.get(entry.placement)?;
            let join = placement.joins.get(entry.join)?;
            join.draws_in(frame, pass, fits).then_some((entry.placement, placement, join))
        });
        drawn.extend(listed);

        drawn
    }
}

/// How many instances a placement draws at a frame: a count known to the CPU, or as many as the five words of
/// `buffer` from the byte `at` hold, read by the GPU.
enum Count<'a> {
    Given(u32),
    Held { buffer: &'a wgpu::Buffer, at: u64 },
}

impl Joined {
    /// Whether it draws in the pass `pass` of `frame`, whose programs are made against `fits`: the pass it joined, of
    /// the shape it was made against, reading what it was made to read.
    fn draws_in(&self, frame: Frame, pass: PassName, fits: &Fits) -> bool {
        self.joins(frame, pass) && self.fits == *fits
    }

    /// Whether it is the join of the pass `pass` of `frame`, whatever that pass is made against now.
    fn joins(&self, frame: Frame, pass: PassName) -> bool {
        self.frame == frame && self.pass == pass
    }
}

impl Placement {
    /// The passes it joins listed, each of its frame.
    fn listed(&self) -> impl Iterator<Item = (Frame, PassName)> {
        self.joins.iter().filter(|join| join.listing != Listing::Loose).map(|join| (join.frame, join.pass))
    }
}

/// What a list's check finds of the placements kept.
impl Placements for Made<Placement> {
    fn listing(
        &mut self,
        placement: u64,
        by: ActorId,
        (frame, pass): (Frame, PassName),
    ) -> Result<(usize, &mut Listing), EntryRefused> {
        let joins = match self.owned(placement, by) {
            Owned::Mine(made) => &mut made.joins,
            Owned::NotOwner => return Err(EntryRefused::NotOwner),
            Owned::Unknown => return Err(EntryRefused::Placement),
        };
        let at = joins.iter().position(|join| join.joins(frame, pass)).ok_or(EntryRefused::Unjoined)?;

        Ok((at, &mut joins[at].listing))
    }
}

impl Dispatching {
    /// Whether it runs in the compute pass `pass` of `frame`: the pass it joined.
    fn runs_in(&self, frame: Frame, pass: PassName) -> bool {
        self.frame == frame && self.pass == pass
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
            Self::Pixels { buffer, .. } | Self::Bytes { buffer } | Self::Stamps { buffer, .. } => buffer,
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

/// The copy of `storage`, of `size` bytes, into a buffer the chassis can map, recorded on `encoder`.
fn copy_storage(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    storage: &wgpu::Buffer,
    size: u32,
) -> Reading {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(size),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_buffer_to_buffer(storage, 0, &buffer, 0, u64::from(size));

    Reading::Bytes { buffer }
}

/// The timestamps a timed frame's passes write as each begins and ends: where the GPU writes them, where it resolves
/// them to, and the buffer they are copied into for the chassis to map, with how many there are.
struct Stamps {
    set: wgpu::QuerySet,
    resolved: wgpu::Buffer,
    read: wgpu::Buffer,
    count: u32,
}

impl Stamps {
    /// Room for the timestamps of `passes` passes, two each.
    fn new(device: &wgpu::Device, passes: u32) -> Self {
        let count = 2 * passes;
        let set =
            device.create_query_set(&wgpu::QuerySetDescriptor { label: None, ty: wgpu::QueryType::Timestamp, count });
        let bytes = u64::from(count) * u64::from(wgpu::QUERY_SIZE);
        let buffer = |usage| {
            device.create_buffer(&wgpu::BufferDescriptor { label: None, size: bytes, usage, mapped_at_creation: false })
        };
        let resolved = buffer(wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC);
        let read = buffer(wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);

        Self { set, resolved, read, count }
    }

    /// Where the pass `at`, counted from zero, writes its timestamps: the first as it begins, the second as it ends.
    const fn writes(&self, at: u32) -> wgpu::RenderPassTimestampWrites<'_> {
        wgpu::RenderPassTimestampWrites {
            query_set: &self.set,
            beginning_of_pass_write_index: Some(2 * at),
            end_of_pass_write_index: Some(2 * at + 1),
        }
    }

    /// Where the compute pass `at`, counted from zero, writes its timestamps, as a pass that draws does.
    const fn compute_writes(&self, at: u32) -> wgpu::ComputePassTimestampWrites<'_> {
        wgpu::ComputePassTimestampWrites {
            query_set: &self.set,
            beginning_of_pass_write_index: Some(2 * at),
            end_of_pass_write_index: Some(2 * at + 1),
        }
    }

    /// Resolves the timestamps and copies them where the chassis can map them, recorded on `encoder` after the
    /// passes, each tick `period` nanoseconds.
    fn resolve(self, encoder: &mut wgpu::CommandEncoder, period: f32) -> Reading {
        let bytes = u64::from(self.count) * u64::from(wgpu::QUERY_SIZE);
        encoder.resolve_query_set(&self.set, 0..self.count, &self.resolved, 0);
        encoder.copy_buffer_to_buffer(&self.resolved, 0, &self.read, 0, bytes);

        Reading::Stamps { buffer: self.read, period }
    }
}

/// What a placement's check finds of what is kept.
impl Finds for Kept {
    fn pass(&self, frame: Frame, pass: PassName) -> Result<Fits, JoinRefused> {
        let walk = match frame {
            Frame::Main(window) => self.main(window).ok_or(JoinRefused::Frame)?,
            Frame::Texture(id) => {
                self.frames.get(id.id).filter(|walk| walk.texture().is_some()).ok_or(JoinRefused::Frame)?
            }
        };

        walk.fits(pass).ok_or(JoinRefused::Pass)
    }

    fn drawn_by(&self, texture: TextureId) -> Option<u64> {
        let Origin::Frame(frame) = self.textures.get(texture.id)?.origin else {
            return None;
        };

        self.frames.get(frame).map(|_| frame)
    }

    fn reads(&self) -> Vec<(u64, u64)> {
        let joined = self
            .placements
            .values()
            .flat_map(|placement| &placement.joins)
            .map(|join| (join.frame, join.textures.as_slice()));
        let dispatched = self.dispatches.values().map(|dispatch| (dispatch.frame, dispatch.textures.as_slice()));
        let bound = self
            .frames
            .iter()
            .filter(|(_, walk)| walk.texture().is_some())
            .map(|(frame, walk)| (Frame::Texture(FrameId { id: frame }), walk.binds()));
        let textures = joined
            .chain(dispatched)
            .chain(bound)
            .filter_map(|(frame, textures)| match frame {
                Frame::Texture(frame) => Some((frame.id, textures)),
                Frame::Main(_) => None,
            })
            .flat_map(|(frame, textures)| textures.iter().map(move |texture| (frame, *texture)))
            .filter_map(|(frame, texture)| Some((frame, self.drawn_by(TextureId { id: texture })?)));
        // Only a use in a frame that ends on a texture makes a pair, and only a read with a write: each storage's
        // readers and writers there are paired, not every use with every other, which a storage bound by every
        // placement of a window's frame would make millions of.
        let mut uses: HashMap<u64, (Vec<Use>, Vec<Use>)> = HashMap::new();
        for (storage, used) in self.storage_uses().filter(|(_, used)| matches!(used.frame, Frame::Texture(_))) {
            let (readers, writers) = uses.entry(storage).or_default();
            match used.access {
                Access::Read => readers.push(used),
                Access::Write => writers.push(used),
            }
        }
        let stored = uses.into_values().flat_map(|(readers, writers)| {
            readers
                .iter()
                .flat_map(|reader| writers.iter().filter_map(|writer| reader.reads_from(*writer)))
                .collect::<Vec<_>>()
        });
        let mut reads: Vec<(u64, u64)> = textures.chain(stored).collect();
        reads.sort_unstable();
        reads.dedup();

        reads
    }

    fn uses(&self, storage: StorageId) -> Vec<Use> {
        self.storage_uses().filter(|(used, _)| *used == storage.id).map(|(_, used)| used).collect()
    }

    fn program(&self, id: ProgramId) -> Option<Reads<'_>> {
        let program = self.programs.get(id.id)?;
        let Stages::Draws { layout, instances, reads } = &program.stages else {
            return None;
        };

        Some(Reads { layout, instances, bindings: &program.bindings, targets: reads, shape: program.shape })
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

impl Kept {
    /// Each use of storage by what is kept, beside the storage's id: each placement's join that binds it, each effect
    /// of a frame, and each dispatch, in no order.
    fn storage_uses(&self) -> impl Iterator<Item = (u64, Use)> {
        let joined = self.placements.values().flat_map(|placement| &placement.joins).flat_map(|join| {
            let used = Use { frame: join.frame, pass: join.pass, access: Access::Read };
            join.storage.iter().map(move |storage| (*storage, used))
        });
        let bound = self.frames.iter().flat_map(|(id, walk)| {
            let frame = walk.frame(id);
            walk.stored()
                .iter()
                .map(move |(pass, storage)| (*storage, Use { frame, pass: *pass, access: Access::Read }))
        });
        let dispatched = self.dispatches.values().flat_map(|dispatch| {
            dispatch.storage.iter().map(|(storage, access)| {
                (*storage, Use { frame: dispatch.frame, pass: dispatch.pass, access: *access })
            })
        });

        joined.chain(bound).chain(dispatched)
    }
}

/// What a dispatch's check finds of what is kept, beside what a placement's does.
impl Dispatches for Kept {
    fn computes(&self, frame: Frame, pass: PassName) -> Result<(), DispatchRefused> {
        let walk = match frame {
            Frame::Main(window) => self.main(window).ok_or(DispatchRefused::Frame)?,
            Frame::Texture(id) => {
                self.frames.get(id.id).filter(|walk| walk.texture().is_some()).ok_or(DispatchRefused::Frame)?
            }
        };
        if !walk.computes(pass) {
            return Err(DispatchRefused::Pass);
        }

        Ok(())
    }

    fn compute(&self, id: ComputeId) -> Option<Computes<'_>> {
        let compute = self.computes.get(id.id)?;

        Some(Computes { bindings: &compute.bindings, workgroup: compute.workgroup, arrays: &compute.arrays })
    }

    fn maker(&self, id: StorageId) -> Option<ActorId> {
        self.storage.maker(id.id)
    }

    fn most_workgroups(&self) -> u32 {
        self.most_workgroups
    }
}

/// What a frame's check finds of what is kept, beside what a placement's does.
impl Holds for Kept {
    fn window(&self, window: WindowId) -> Option<ColourFormat> {
        self.windowed(window).map(|windowed| windowed.format)
    }

    fn longest(&self) -> u32 {
        self.longest
    }

    fn effect(&self, id: ProgramId) -> Option<EffectReads<'_>> {
        let program = self.programs.get(id.id)?;
        let Stages::Effect { reads } = &program.stages else {
            return None;
        };

        Some(EffectReads { reads, bindings: &program.bindings, shape: program.shape })
    }
}

/// An effect pass of the renderer's own main frame, which has none.
const fn effectless(_: Making<'_>) -> Result<walk::Effected, PassRefused> {
    Err(PassRefused::Effect)
}

/// The textures `bindings` binds, by their ids.
fn textures(bindings: &[Bind]) -> Vec<u64> {
    bindings
        .iter()
        .filter_map(|bind| match bind.to {
            placement::Resource::Texture(texture) => Some(texture.id),
            placement::Resource::Uniforms(_) | placement::Resource::Storage(_) | placement::Resource::Sampler(_) => {
                None
            }
        })
        .collect()
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
        self.kept_for(turn, |kept, maker| {
            let id = kept.ids.next();
            keep(kept, id, maker);
            id
        })
    }

    /// What `keep` makes of what is kept for the sender of the mail being handled, watching the sender when it is
    /// the first time it has made anything, so what it made is released when it ends.
    fn kept_for<S, T>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        keep: impl FnOnce(&mut Kept, ActorId) -> T,
    ) -> Result<T, Fault> {
        let maker = turn.sender();
        let kept = turn.open(&mut self.kept)?;
        let made = keep(kept, maker.id());
        let first = kept.makers.made(maker.id());
        if first {
            turn.watch(maker);
        }

        Ok(made)
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

/// The pipeline `create` asks for, of the WGSL `wgsl` its blob holds, drawing into passes of its shape, made from what
/// its check let through.
fn pipeline(gpu: &Gpu, create: &CreateProgram, wgsl: &str) -> Program {
    let device = gpu.device();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    let mut groups = groups(device, &create.bindings, wgpu::ShaderStages::VERTEX_FRAGMENT);
    // One that reads targets binds them in its first group, where it lists nothing; one that reads none keeps it.
    if !create.reads.is_empty() {
        first_reads(&mut groups, read_layout(device, &create.reads));
    }
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
    let target =
        wgpu::ColorTargetState { format: create.shape.colour.wgpu(), blend, write_mask: wgpu::ColorWrites::ALL };
    let depth_stencil = match create.shape.depth {
        DepthFormat::None => None,
        DepthFormat::Depth32Float => Some(wgpu::DepthStencilState {
            format: DEPTH,
            depth_write_enabled: Some(writes),
            depth_compare: Some(compare),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
    };
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
        depth_stencil,
        multisample: wgpu::MultisampleState {
            count: create.shape.samples.count(),
            alpha_to_coverage_enabled: create.state.coverage == Coverage::Alpha,
            ..Default::default()
        },
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
        stages: Stages::Draws {
            layout: create.layout.clone(),
            instances: create.instances.clone(),
            reads: create.reads.clone(),
        },
        bindings: create.bindings.clone(),
        groups,
        shape: create.shape,
    }
}

/// The depth a pass holds beside its colour, when its shape holds any.
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The pipeline of the effect `create` asks for: the renderer's vertex stage, one triangle over the whole target, and
/// its fragment stage, drawing into targets of its shape, reading its targets in its first group, empty when it reads
/// none, and binding what it lists in the groups after, of the WGSL `wgsl` its blob holds; made from what its check
/// let through.
fn effect_pipeline(device: &wgpu::Device, create: &CreateEffect, wgsl: &str) -> Program {
    let cover = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(COVER.into()),
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    let mut groups = groups(device, &create.bindings, wgpu::ShaderStages::VERTEX_FRAGMENT);
    first_reads(&mut groups, read_layout(device, &create.reads));
    let bound: Vec<Option<&wgpu::BindGroupLayout>> = groups.iter().map(Some).collect();
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &bound,
        immediate_size: 0,
    });
    let target = wgpu::ColorTargetState {
        format: create.shape.colour.wgpu(),
        blend: blending(create.blend),
        write_mask: wgpu::ColorWrites::ALL,
    };
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &cover,
            entry_point: Some(COVER_ENTRY),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState { count: create.shape.samples.count(), ..Default::default() },
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
        stages: Stages::Effect { reads: create.reads.clone() },
        bindings: create.bindings.clone(),
        groups,
        shape: create.shape,
    }
}

/// The pipeline of the compute program `create` asks for, of the WGSL `wgsl` its blob holds, with what its check read
/// of its shader, made from what its check let through.
fn compute_pipeline(device: &wgpu::Device, create: &CreateCompute, wgsl: &str, checked: compute::Checked) -> Compute {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    let groups = groups(device, &create.bindings, wgpu::ShaderStages::COMPUTE);
    let bound: Vec<Option<&wgpu::BindGroupLayout>> = groups.iter().map(Some).collect();
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &bound,
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &module,
        entry_point: Some(&create.entry),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });

    Compute {
        pipeline,
        bindings: create.bindings.clone(),
        groups,
        workgroup: checked.workgroup,
        arrays: checked.arrays,
    }
}

/// The layout of the group a pass's targets are read in, as `reads` says, the first at binding 0, by the fragment
/// stage: one layout for an effect's reads and a draw program's, and for the group the pass binds them in, so each is
/// bound through another made alike (ADR-0025).
fn read_layout(device: &wgpu::Device, reads: &[Read]) -> wgpu::BindGroupLayout {
    let entries: Vec<wgpu::BindGroupLayoutEntry> = (0..)
        .zip(reads)
        .map(|(binding, read)| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: sample_type(*read),
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        })
        .collect();

    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &entries })
}

/// Puts `reads`, the layout of what a program reads, as the first of `groups`, the layouts of what it lists from group
/// 1, whose first is empty as it lists nothing there, or the only one when it lists nothing.
fn first_reads(groups: &mut Vec<wgpu::BindGroupLayout>, reads: wgpu::BindGroupLayout) {
    match groups.first_mut() {
        Some(first) => *first = reads,
        None => groups.push(reads),
    }
}

/// How a target read so is sampled, as wgpu names it.
const fn sample_type(read: Read) -> wgpu::TextureSampleType {
    match read {
        Read::Float => wgpu::TextureSampleType::Float { filterable: true },
        Read::Uint => wgpu::TextureSampleType::Uint,
        Read::Depth => wgpu::TextureSampleType::Depth,
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
/// skip; each binding is seen by the stages `visibility` names.
fn groups(device: &wgpu::Device, bindings: &[Binding], visibility: wgpu::ShaderStages) -> Vec<wgpu::BindGroupLayout> {
    let count = bindings.iter().map(|binding| binding.group + 1).max().unwrap_or(0);

    (0..count)
        .map(|group| {
            let entries: Vec<wgpu::BindGroupLayoutEntry> = bindings
                .iter()
                .filter(|binding| binding.group == group)
                .map(|binding| wgpu::BindGroupLayoutEntry {
                    binding: binding.binding,
                    visibility,
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
        BindingKind::Storage { bytes, access } => wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: access == Access::Read },
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
