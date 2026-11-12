//! The renderer: the root that owns the GPU, makes what actors ask it for, and draws each frame (ADR-0021).

mod groups;
mod picking;
// The tests draw on a device that draws nothing, which only a native build has.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
mod text;
mod walk;
mod watching;

use crate::Gpu;
use crate::compute::{self, Array, ComputeId, Computed, CreateCompute};
use crate::dispatch::{
    self, Computes, CreateDispatch, DispatchId, DispatchRan, DispatchRefused, Dispatched, Dispatches, Grid, Runs,
    Unrun, Use,
};
use crate::draw_list::{Cut, DrawList, ListRefused, Listables, Listed, Lists, Picks, Walking, Within};
use crate::effect::{self, COVER, COVER_ENTRY, CreateEffect, Effected, MadeEffect, Read};
use crate::frame::{
    self, CreateFrame, DepthUse, Does, EffectReads, Ends, Frame, FrameId, FrameRefused, Framed, Holds, Load,
    PassRefused, ShapeOf, Shaped, Size, Target, TargetKind,
};
use crate::instances::{self, CreateInstances, Instanced, InstancesId, WriteInstances};
use crate::made::{Ids, Made, Makers, Owned, Weighed, Weighs, reweighed};
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
use crate::readback::{
    self, Copied, Order, Pixels, ReadBack, ReadFrameTarget, ReadRefused, ReadStorage, Readable, Region,
    StorageContents, TargetPixels,
};
use crate::shape::{ColourFormat, DepthFormat, Samples, Shape};
use crate::stats::{
    self, DescribeGpu, FrameDrawing, FrameStats, GpuDescribed, GpuMemory, GpuTimes, Ran, SubscribeFrameStats,
    SubscribeGpuTimes, Tally, UnsubscribeFrameStats, UnsubscribeGpuTimes,
};
use crate::storage::{self, CreateStorage, CreateZeroedStorage, StorageId, Stored, WriteStorage};
use crate::surface::{Front, Present, Surfaced, WindowFormat};
use crate::text::faces::Faces;
use crate::text::generate::{Generating, GlyphsGenerated};
use crate::text::{
    CreateFont, CreateText, FontId, FontMade, FontMetrics, MoveText, PlaceText, ReadMetrics, TextId, TextMade,
    TextPlaced, WriteText,
};
use crate::texture::{self, CreateTexture, TextureFormat, TextureId, Textured};
use crate::timing::{self, TimeFrame, Timed, TimingRefused};
use crate::uniforms::{self, CreateUniforms, Uniformed, UniformsId, WriteUniforms};
use aethereal_actor::{
    Actor, Allows, Answer, Beginning, Bytes, Carries, ChildOf, Controller, Ended, Ending, Fault, Given, Handle,
    Handles, Held, Level, On, Path, Protocol, ProtocolRef, Render, Resource, Source, Subscribers, Tells, ToAny,
    ToSender, Top, Turn, path,
};
use aethereal_data::{ActorId, Bound, PathSegment, Plain};
use aethereal_math::{Rect2, Vec2};
use aethereal_window::{Hearer, KeyHearer, KeysEntry, KeysListed, Picked, Probed, SubscribeProbes, Window, WindowId};
use groups::{Binds, Entry, Groups};
use picking::{InPass, Picker, Picking, Probing, Stacks};
use std::collections::HashMap;
use std::mem;
use text::{PlacedText, TextJoined, Texts};
use walk::{End, Making, ReadGroup, Step, Walk};
use watching::Watch;

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
    /// Who is sent each frame's statistics, and what is counted for them meanwhile (ADR-0053).
    watch: Watch,
    /// Who is sent the GPU's time over each frame's passes (ADR-0053).
    timed: Subscribers<(GpuTimes,)>,
}

/// The reads, timings, and dispatches the renderer owes: those of the next frame, each read under the window it reads,
/// those copied and waiting on their buffer's map, each under the read the buffer is kept by: pixels, storage's bytes,
/// and timings, and the maker to tell of each dispatch that runs once and has not, under its id.
#[derive(Bound)]
struct Owed {
    frames: Vec<(WindowId, Held<Pixels>)>,
    timings: Vec<Held<Timed>>,
    reading: Vec<(u64, Held<Pixels>)>,
    targets: Vec<(u64, Held<TargetPixels>)>,
    storing: Vec<(u64, Held<StorageContents>)>,
    timing: Vec<(u64, Held<Timed>)>,
    dispatches: Vec<(u64, ProtocolRef<(DispatchRan,)>)>,
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

impl Carries<CreateZeroedStorage> for Renderer {
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

impl Carries<ReadFrameTarget> for Renderer {
    type Answers = (TargetPixels,);
}

impl Carries<ReadStorage> for Renderer {
    type Answers = (StorageContents,);
}

impl Carries<TimeFrame> for Renderer {
    type Answers = (Timed,);
}

impl Carries<DescribeGpu> for Renderer {
    type Answers = (GpuDescribed,);
}

/// What the renderer is made from: the GPU the chassis opened; the front the chassis hands it each window's frame
/// through, with a slot for each window's surface it keeps ([`WindowSurface::new`](crate::surface::WindowSurface::new)),
/// and the controller it asks the chassis to present each window's frame it drew through; each window it draws to,
/// with the format of the window's surface, in the order the chassis opened them; the controller it asks the chassis
/// to map what it reads back through, and the source the chassis brings word of each through; the source the
/// chassis brings word of each window it opens or closes while the program runs through (ADR-0019, ADR-0021,
/// ADR-0027); and the program's root list, the id of each root whose list each window's main frame is walked from, in
/// draw order, as its setup reads it (ADR-0055).
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
    pub roots: Vec<ActorId>,
}

/// What an actor asks the renderer: to make a mesh, a texture, a program, a compute program, an effect, instances,
/// uniforms, storage, or a frame, to write instances, uniforms, or storage it made, to place what is drawn or dispatch
/// what is computed, until released or once, to list what it draws in order, the shape of a pass it joins,
/// to release what it made, to read back what it draws or storage, to time what it draws, to make a font or read its
/// metrics, to make, write, place, or move a text (ADR-0052), or to be sent each frame's statistics or the
/// GPU's time over its passes, or told the adapter it draws on (ADR-0053).
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
        CreateZeroedStorage,
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
        CreateFont,
        ReadMetrics,
        CreateText,
        WriteText,
        PlaceText,
        MoveText,
        SubscribeFrameStats,
        UnsubscribeFrameStats,
        SubscribeGpuTimes,
        UnsubscribeGpuTimes,
        DescribeGpu,
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

impl Carries<CreateZeroedStorage> for Rendering {
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

impl Carries<ReadFrameTarget> for Rendering {
    type Answers = (TargetPixels,);
}

impl Carries<ReadStorage> for Rendering {
    type Answers = (StorageContents,);
}

impl Carries<TimeFrame> for Rendering {
    type Answers = (Timed,);
}

impl Carries<CreateFont> for Rendering {
    type Answers = (FontMade,);
}

impl Carries<ReadMetrics> for Rendering {
    type Answers = (FontMetrics,);
}

impl Carries<CreateText> for Rendering {
    type Answers = (TextMade,);
}

impl Carries<WriteText> for Rendering {
    type Answers = ();
}

impl Carries<PlaceText> for Rendering {
    type Answers = (TextPlaced,);
}

impl Carries<MoveText> for Rendering {
    type Answers = ();
}

impl Carries<SubscribeFrameStats> for Rendering {
    type Answers = ();
}

impl Carries<UnsubscribeFrameStats> for Rendering {
    type Answers = ();
}

impl Carries<SubscribeGpuTimes> for Rendering {
    type Answers = ();
}

impl Carries<UnsubscribeGpuTimes> for Rendering {
    type Answers = ();
}

impl Carries<DescribeGpu> for Rendering {
    type Answers = (GpuDescribed,);
}

/// Releases what the renderer made before its maker ends. Only its maker may; what another asks is logged and left.
/// A text released takes its placements with it, whoever made them (ADR-0052).
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
    Font(FontId),
    Text(TextId),
}

/// What the renderer has made, and who made each, with the samplers its placements, dispatches, and effects share, the
/// lists each actor drew its placements in a pass by, the frames that end on textures in the order they run, `order`,
/// the longest side of a texture and the most workgroups along one dimension the GPU makes and dispatches, each window
/// it draws to, in the order its chassis opened them, with the GPU memory they hold, `windows_memory`, and its fonts,
/// texts, and texts placed, with their atlas. What holds GPU memory is weighed as it is made, made again, and released
/// (ADR-0053).
struct Kept {
    ids: Ids,
    makers: Makers,
    meshes: Weighed<Mesh>,
    textures: Weighed<Texture>,
    programs: Made<Program>,
    computes: Made<Compute>,
    instances: Weighed<Instances>,
    uniforms: Weighed<Uniforms>,
    storage: Weighed<Storage>,
    placements: Made<Placement>,
    dispatches: Made<Dispatching>,
    lists: Lists,
    reads: HashMap<u64, Reading>,
    samplers: HashMap<Sampling, wgpu::Sampler>,
    frames: Weighed<Walk>,
    order: Vec<u64>,
    /// The dispatches not run: each writes a storage another writes in a frame it is in no order with.
    held: Vec<u64>,
    longest: u32,
    most_workgroups: u32,
    windows: Vec<Windowed>,
    /// The GPU memory of each window's own main frame's targets and the copy of its last frame, counted as each window
    /// is opened, fitted, drawn, and closed.
    windows_memory: GpuMemory,
    text: Texts,
    picker: Picker,
}

/// A window the renderer draws to: the window, the format of its surface, the renderer's own main frame of it, `own`,
/// which frame is its main frame, `main`, the last frame drawn onto it, kept, its probes, and the `Keys` entries its
/// actor was last sent, the last called first (ADR-0055).
struct Windowed {
    window: WindowId,
    format: ColourFormat,
    own: Walk,
    main: Main,
    last: Last,
    probing: Probing,
    keys: Vec<KeysEntry>,
}

/// The last frame drawn onto a window, kept so it is read while no frame is drawn, as under a held clock (ADR-0045).
/// One texture, made at the frame's size and format and copied into each `Render`, made again only when either
/// changes.
enum Last {
    /// None is kept: the window has not been drawn onto, or its surface cannot be copied from.
    Undrawn,
    Kept(wgpu::Texture),
}

impl Last {
    /// Copies `frame`, just drawn, into the texture kept, made again when it is not of the frame's size and format, on
    /// `encoder`. A frame that cannot be copied from keeps nothing.
    fn keep(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, frame: &wgpu::Texture) {
        let copyable = frame.usage().contains(wgpu::TextureUsages::COPY_SRC);
        if !copyable {
            *self = Self::Undrawn;
            return;
        }
        let fits = match self {
            Self::Kept(kept) => kept.size() == frame.size() && kept.format() == frame.format(),
            Self::Undrawn => false,
        };
        if !fits {
            *self = Self::Kept(device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: frame.size(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: frame.format(),
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            }));
        }
        let Self::Kept(kept) = self else {
            return;
        };
        encoder.copy_texture_to_texture(frame.as_image_copy(), kept.as_image_copy(), frame.size());
    }
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

        Ok(Self {
            window,
            format,
            own,
            main: Main::Own,
            last: Last::Undrawn,
            probing: Probing::new(device),
            keys: Vec::new(),
        })
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
    /// A region of a frame's target, laid out as `kind` holds it, lying `within` what was copied so.
    Target { buffer: wgpu::Buffer, copied: Copied, within: Region, region: Region, kind: TargetKind },
    /// Storage's bytes, copied whole.
    Bytes { buffer: wgpu::Buffer },
    /// A pass's timestamps, each tick `period` nanoseconds, and whether they are named for subscribers.
    Stamps { buffer: wgpu::Buffer, period: f32, named: Named },
    /// A window's stacks at its probes, as a frame drew them (ADR-0055).
    Picks(Stacks),
}

/// Whose a frame's timestamps are: only those who asked [`TimeFrame`], or also the subscribers of [`GpuTimes`], told
/// them each beside its frame and its pass's name, of the cycle at `elapsed_nanos` (ADR-0053).
enum Named {
    Unnamed,
    Named { elapsed_nanos: u64, passes: Vec<Ran> },
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
    /// the pass it joins as `reads` says, bound in its first group when it reads any, binding the picking step in the
    /// group `picking`, after its own (ADR-0055).
    Draws { layout: VertexLayout, instances: PerInstance, reads: Vec<Read>, picking: u32 },
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

/// One pass a placement draws in, with what it draws there with: the program's id, which a pass sets a pipeline anew
/// only when it changes by, its pipeline and what it was made against, the bind
/// groups made for it, from group 1 when the program reads targets of the pass and from group 0 when it reads none,
/// the group its program binds the picking step in, and the textures and storage they bind, by their ids, with the
/// storage the placement reads its counts from.
struct Joined {
    frame: Frame,
    pass: PassName,
    fits: Fits,
    program: u64,
    pipeline: wgpu::RenderPipeline,
    groups: Groups,
    picking: u32,
    textures: Vec<u64>,
    storage: Vec<u64>,
}

/// One dispatch run in each cycle that draws its frame with it listed, until released or once, holding all it runs
/// with, so a frame builds nothing and releasing what it was made of frees it only once the dispatch goes (ADR-0026):
/// the compute pass it joins, how long it runs, its compute program's id, which a pass runs dispatches together by,
/// and its pipeline, the bind groups made for it, the workgroups it runs, and the textures and storage it binds or reads
/// its workgroups from, by their ids, the storage with how it is used.
struct Dispatching {
    frame: Frame,
    pass: PassName,
    runs: Runs,
    compute: u64,
    pipeline: wgpu::ComputePipeline,
    groups: Groups,
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
    size: u64,
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
        CreateZeroedStorage,
        WriteStorage,
        Place,
        CreateDispatch,
        DrawList,
        ShapeOf,
        CreateEffect,
        CreateFrame,
        Release,
        ReadBack,
        ReadFrameTarget,
        ReadStorage,
        TimeFrame,
        Mapped,
        Surfaced,
        Ended,
        CreateFont,
        ReadMetrics,
        CreateText,
        WriteText,
        PlaceText,
        MoveText,
        Answer<GlyphsGenerated, Generating>,
        SubscribeFrameStats,
        UnsubscribeFrameStats,
        SubscribeGpuTimes,
        UnsubscribeGpuTimes,
        DescribeGpu,
        Probed,
    );
    type BeginSends = (Tells<Path<Window>, SubscribeProbes>,);
    type EndSends = ();

    fn begin(config: RendererConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        // The renderer's own fonts, their metrics put once, as blobs kept for the renderer's life (ADR-0052).
        let faces = match Faces::new(|bytes| turn.put(bytes)) {
            Ok(faces) => faces,
            Err(why) => return Err(Fault::new(&why)?),
        };
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
        let probed: Vec<WindowId> = windows.iter().map(|windowed| windowed.window).collect();
        // Made before it is kept, as keeping it ends the borrow of the GPU opened.
        let kept = Kept::new(opened.device(), windows, config.roots, faces);
        let kept = turn.keep(kept)?;
        turn.subscribe::<Render>();
        for window in probed {
            turn.to(path::<Window>(window)).send(&SubscribeProbes)?;
        }

        let owed = Owed {
            frames: Vec::new(),
            timings: Vec::new(),
            reading: Vec::new(),
            targets: Vec::new(),
            storing: Vec::new(),
            timing: Vec::new(),
            dispatches: Vec::new(),
        };

        Ok(Self {
            gpu,
            kept,
            front,
            presenting,
            mapping,
            mapped,
            surfaced,
            owed,
            watch: Watch::Unwatched,
            timed: Subscribers::new(),
        })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// Draws each window handed a frame, sends each of them the `Keys` entries its walk holds when they have changed since
/// it was last sent them (ADR-0055), and tells each subscriber of the frame's statistics, counted only while one
/// subscribes, and of its passes' times, timed only while one subscribes (ADR-0053). A cycle that draws nothing tells
/// the subscribers of its statistics so, and its passes have no time to tell. A frame not drawn runs none of its
/// dispatches (ADR-0026), and the maker of each dispatch that runs once and ran is told so once the cycle's commands are
/// submitted.
impl Handles<On<Render>> for Renderer {
    type Sends =
        (ToAny<(FrameStats,), FrameStats>, Tells<Path<Window>, KeysListed>, ToAny<(DispatchRan,), DispatchRan>);

    fn handle(&mut self, render: Render, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let front = turn.open(&mut self.front)?;
        let handed = front.take();
        let budget = front.budget();
        let gpu = turn.open(&mut self.gpu)?;
        // Held past the opening of what is kept, as a turn opens one resource at a time.
        let (device, queue) = (gpu.device().clone(), gpu.queue().clone());
        let kept = turn.open(&mut self.kept)?;
        // A window the chassis handed no frame for is not drawn this cycle, nor asked to be presented. A cycle it handed
        // none for at all is skipped: nothing is drawn, the next frame drawn is the one read and timed, and the
        // subscribers hear that this one drew nothing, so a read of the statistics is answered while no window is. A
        // frame owed this cycle is refused, as none is drawn: under a held clock no cycle follows to draw it in.
        let frames = kept.ordered(handed);
        if frames.is_empty() {
            if let Watch::Watched { subscribers, .. } = &self.watch {
                let undrawn = kept.frame_stats(render.elapsed_nanos, budget, FrameDrawing::Undrawn);
                turn.to(subscribers).send(&undrawn)?;
            }
            for (_, held) in mem::take(&mut self.owed.frames) {
                turn.answer(held, &Pixels::Refused(ReadRefused::Unshown))?;
            }
            return Ok(());
        }
        let views: Vec<(WindowId, wgpu::TextureView)> = frames
            .iter()
            .map(|(window, frame)| (*window, frame.create_view(&wgpu::TextureViewDescriptor::default())))
            .collect();
        for (window, frame) in &frames {
            kept.fit(&device, *window, Size { width: frame.width(), height: frame.height() });
        }
        let wanted = kept.ready_texts((&device, &queue));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let passes = kept.passes(&views);
        let asked = !self.owed.timings.is_empty();
        let subscribed = !self.timed.references().is_empty();
        let timed = asked || subscribed;
        let stamps = timed.then(|| Stamps::new(&device, passes));
        let named = if subscribed {
            Named::Named { elapsed_nanos: render.elapsed_nanos, passes: kept.pass_names(&views) }
        } else {
            Named::Unnamed
        };
        let mut tally = self.watch.tally();
        let (once, stacks, past) = kept.run_all(&mut encoder, (&device, &queue), &views, stamps.as_ref(), &mut tally);
        kept.keep_last(&device, &mut encoder, &frames);
        let copied: Vec<_> = frames
            .iter()
            .filter_map(|(window, frame)| {
                let owed = owed_by(&mut self.owed.frames, *window);
                let asked = !owed.is_empty();
                asked.then(|| (copy_frame(&device, &mut encoder, frame), owed))
            })
            .collect();
        let timed = stamps.map(|stamps| stamps.resolve(&mut encoder, queue.get_timestamp_period(), named));
        queue.submit([encoder.finish()]);
        kept.ran_once(&once);
        let stats = match tally {
            Tally::On(drawn) => Some(kept.frame_stats(render.elapsed_nanos, budget, FrameDrawing::Drawn(drawn))),
            Tally::Off => None,
        };

        let mut mappings = Vec::new();
        let mut refused = Vec::new();
        for (copy, owed) in copied {
            match copy {
                Ok(reading) => {
                    let (read, mapping) = kept.reading(reading);
                    mappings.push(mapping);
                    self.owed.reading.extend(owed.into_iter().map(|held| (read, held)));
                }
                Err(why) => refused.extend(owed.into_iter().map(|held| (held, why))),
            }
        }
        for stacks in stacks {
            let (_, mapping) = kept.reading(Reading::Picks(stacks));
            mappings.push(mapping);
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
        if past > 0 {
            turn.log(Level::Error, format_args!("{past} draws were not drawn: past the most calls a `Render` binds"));
        }
        self.tell_ran(turn, once)?;
        self.relist(turn, frames.iter().map(|(window, _)| *window))?;
        // Pushed last, so a turn that fails before it asks the chassis for nothing (ADR-0019).
        let control = turn.open(&mut self.mapping)?;
        for mapping in mappings {
            control.push(mapping);
        }
        if let Some((job, generating)) = wanted {
            turn.run(&job, &generating)?;
        }
        if let (Some(stats), Watch::Watched { subscribers, .. }) = (stats, &self.watch) {
            turn.to(subscribers).send(&stats)?;
        }

        Ok(())
    }
}

impl Handles<ReadBack> for Renderer {
    type Sends = (ToSender<Pixels>,);

    fn handle(&mut self, read: ReadBack, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let gpu = turn.open(&mut self.gpu)?;
        let (device, queue) = (gpu.device().clone(), gpu.queue().clone());
        let kept = turn.open(&mut self.kept)?;
        let texture = match read.of {
            Readable::Texture(texture) => texture,
            Readable::Frame(window) if kept.windowed(window).is_none() => {
                turn.reply(&Pixels::Refused(ReadRefused::Window))?;
                return Ok(());
            }
            Readable::Frame(window) => {
                // Read when the window's next frame is drawn.
                let held = turn.hold()?;
                self.owed.frames.push((window, held));
                return Ok(());
            }
            Readable::Last(window) => {
                let last = match kept.windowed(window).map(|windowed| &windowed.last) {
                    None => Err(ReadRefused::Window),
                    Some(Last::Undrawn) => Err(ReadRefused::Undrawn),
                    Some(Last::Kept(last)) => Ok(last.clone()),
                };
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                let reading = match last.and_then(|last| copy_frame(&device, &mut encoder, &last)) {
                    Ok(reading) => reading,
                    Err(why) => {
                        turn.reply(&Pixels::Refused(why))?;
                        return Ok(());
                    }
                };
                queue.submit([encoder.finish()]);
                let (read, mapping) = kept.reading(reading);
                let held = turn.hold()?;
                self.owed.reading.push((read, held));
                turn.open(&mut self.mapping)?.push(mapping);
                return Ok(());
            }
        };
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

/// Copies the region asked of the frame's target now, as the last `Render` left it, and answers it once the chassis has
/// mapped the copy (ADR-0048).
impl Handles<ReadFrameTarget> for Renderer {
    type Sends = (ToSender<TargetPixels>,);

    fn handle(&mut self, read: ReadFrameTarget, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let gpu = turn.open(&mut self.gpu)?;
        let (device, queue) = (gpu.device().clone(), gpu.queue().clone());
        let kept = turn.open(&mut self.kept)?;
        let ReadFrameTarget { frame, target, region } = read;
        let refused = kept.copyable(frame, target, region);
        let (kind, region, texture) = match refused {
            Ok(copied) => copied,
            Err(why) => {
                turn.reply(&TargetPixels::Refused(why))?;
                return Ok(());
            }
        };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let reading = copy_region(&device, &mut encoder, &texture, region, kind);
        queue.submit([encoder.finish()]);
        let (read, mapping) = kept.reading(reading);
        let held = turn.hold()?;
        self.owed.targets.push((read, held));
        turn.open(&mut self.mapping)?.push(mapping);

        Ok(())
    }
}

impl Handles<ReadStorage> for Renderer {
    type Sends = (ToSender<StorageContents>,);

    fn handle(&mut self, read: ReadStorage, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let gpu = turn.open(&mut self.gpu)?;
        let (device, queue) = (gpu.device().clone(), gpu.queue().clone());
        let kept = turn.open(&mut self.kept)?;
        let Some((stored, size)) = kept.storage.get(read.storage.id).map(|kept| (kept.buffer.clone(), kept.size))
        else {
            turn.reply(&StorageContents::Refused(ReadRefused::Storage))?;
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

/// Takes in a pointer event's place that a window's actor sent, to probe at the next frame drawn onto the window
/// (ADR-0055). One from any other actor, or of a window the renderer does not draw to, is dropped.
impl Handles<Probed> for Renderer {
    type Sends = ();

    fn handle(&mut self, probed: Probed, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let window = turn.found(&path::<Window>(probed.window));
        let sender = turn.sender();
        let is_window = window.is_some_and(|window| window.erase() == sender);
        if !is_window {
            turn.log(Level::Warn, format_args!("{probed:?} was dropped: only its window's actor probes it"));
            return Ok(());
        }
        let kept = turn.open(&mut self.kept)?;
        if let Some(windowed) = kept.windows.iter_mut().find(|windowed| windowed.window == probed.window) {
            windowed.probing.heard(&probed);
        }

        Ok(())
    }
}

impl Handles<Mapped> for Renderer {
    type Sends = (ToAny<(GpuTimes,), GpuTimes>, Tells<Path<Window>, Picked>);

    fn handle(&mut self, mapped: Mapped, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
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
            Reading::Target { buffer, copied, within, region, kind } => {
                let pixels = ready.then(|| mapped_bytes(&buffer, |bytes| copied.cut(bytes, within))).flatten();
                let answer = match pixels.map(|pixels| turn.put(pixels)) {
                    Some(Ok(pixels)) => TargetPixels::Read { region, kind, pixels },
                    Some(Err(_)) => TargetPixels::Refused(ReadRefused::Unstored),
                    None => TargetPixels::Refused(ReadRefused::Unmapped),
                };
                for held in owed_by(&mut self.owed.targets, read) {
                    turn.answer(held, &answer)?;
                }
            }
            Reading::Bytes { buffer } => {
                let bytes = ready.then(|| mapped_bytes(&buffer, <[u8]>::to_vec)).flatten();
                let answer = match bytes.map(|bytes| turn.put(bytes)) {
                    Some(Ok(bytes)) => StorageContents::Read { bytes },
                    Some(Err(_)) => StorageContents::Refused(ReadRefused::Unstored),
                    None => StorageContents::Refused(ReadRefused::Unmapped),
                };
                for held in owed_by(&mut self.owed.storing, read) {
                    turn.answer(held, &answer)?;
                }
            }
            Reading::Stamps { buffer, period, named } => {
                let passes = ready.then(|| mapped_bytes(&buffer, |bytes| timing::passes(bytes, period))).flatten();
                if let (Some(nanos), Named::Named { elapsed_nanos, passes: names }) = (&passes, named) {
                    let times = GpuTimes::Took { elapsed_nanos, passes: stats::named(names, nanos) };
                    turn.to(&self.timed).send(&times)?;
                }
                let answer = passes.map_or(Timed::Refused(TimingRefused::Unmapped), |passes| Timed::Took { passes });
                for held in owed_by(&mut self.owed.timing, read) {
                    turn.answer(held, &answer)?;
                }
            }
            // A frame's stacks that fail to map are not sent: the next frame's carry the events they would have.
            Reading::Picks(stacks) => {
                let picked = ready.then(|| mapped_bytes(&stacks.buffer, |bytes| stacks.picked(bytes))).flatten();
                if let Some((window, picked)) = picked {
                    turn.to(path::<Window>(window)).send(&picked)?;
                }
            }
        }

        Ok(())
    }
}

/// What the chassis said of a window it opened or closed while the program runs, brought from the source to itself: a
/// window opened is drawn to from the next `Render` it is handed a frame for, by the renderer's own main frame of it,
/// and its probes are asked for,
/// and a window closed has its main frame dropped, with the targets made for it, so what joined only that frame is
/// drawn nowhere until its maker releases it (ADR-0027).
impl Handles<Surfaced> for Renderer {
    type Sends = (Tells<Path<Window>, SubscribeProbes>, ToAny<(DispatchRan,), DispatchRan>);

    fn handle(&mut self, surfaced: Surfaced, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
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
                    Ok(windowed) => {
                        kept.opened(windowed);
                        turn.to(path::<Window>(window)).send(&SubscribeProbes)?;
                    }
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
                for read in owed_by(&mut self.owed.frames, window) {
                    turn.answer(read, &Pixels::Refused(ReadRefused::Window))?;
                }
                self.tell_unjoined(turn)?;
            }
        }

        Ok(())
    }
}

/// What `read`, a read, a window, or a dispatch, owes of `owed`, taken from it.
fn owed_by<K: Copy + PartialEq, T>(owed: &mut Vec<(K, T)>, read: K) -> Vec<T> {
    owed.extract_if(.., |(owing, _)| *owing == read).map(|(_, owing)| owing).collect()
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

        self.watch.uploaded(vertices.len() + indices.len());
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
        self.watch.uploaded(pixels.len());
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
        let checked = program::source(&bytes).and_then(|wgsl| program::check(&create, wgsl, &limits));
        let wgsl = match checked {
            Ok(wgsl) => wgsl,
            Err(why) => {
                turn.reply(&Programmed::Refused(why))?;
                return Ok(());
            }
        };

        let picking = turn.open(&mut self.kept)?.picker.layout().clone();
        let gpu = turn.open(&mut self.gpu)?;
        let made = pipeline(gpu, &create, &wgsl, &picking);
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
        self.watch.uploaded(records.len());
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
        let written = kept.instances.owned(write.instances.id, by, |instances| {
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
        self.watch.uploaded(records.len());

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

        self.watch.uploaded(bytes.len());
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
        let written = kept.uniforms.owned(write.uniforms.id, by, |uniforms| {
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
        self.watch.uploaded(bytes.len());

        Ok(())
    }
}

impl Handles<CreateStorage> for Renderer {
    type Sends = (ToSender<Stored>,);

    fn handle(&mut self, create: CreateStorage, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let bytes = turn.read(&create.bytes).to_vec();
        self.watch.uploaded(bytes.len());
        self.store(turn, bytes.len() as u64, |gpu| upload(gpu, &bytes, STORAGE_USAGE))
    }
}

impl Handles<CreateZeroedStorage> for Renderer {
    type Sends = (ToSender<Stored>,);

    fn handle(&mut self, create: CreateZeroedStorage, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        // WebGPU makes a buffer with every byte zero.
        self.store(turn, create.bytes, |gpu| {
            gpu.device().create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: create.bytes,
                usage: STORAGE_USAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        })
    }
}

impl Handles<WriteStorage> for Renderer {
    type Sends = ();

    fn handle(&mut self, write: WriteStorage, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let by = turn.sender().id();
        let bytes = turn.read(&write.bytes).to_vec();
        let kept = turn.open(&mut self.kept)?;
        let written = kept.storage.owned(write.storage.id, by, |storage| {
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

        turn.open(&mut self.gpu)?.queue().write_buffer(&buffer, write.first, &bytes);
        self.watch.uploaded(bytes.len());

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

/// A dispatch, checked now and kept until released, or, when it runs once, until a cycle runs it or its frame goes, its
/// maker kept to tell (ADR-0026).
impl Handles<CreateDispatch> for Renderer {
    type Sends = (ToSender<Dispatched>,);

    fn handle(&mut self, create: CreateDispatch, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let sender = turn.sender();
        let by = sender.id();
        let told = match create.runs {
            Runs::UntilReleased => Told::Untold,
            Runs::Once => {
                let Some(maker) = turn.cast::<(DispatchRan,)>(sender) else {
                    turn.reply(&Dispatched::Refused(DispatchRefused::Untold))?;
                    return Ok(());
                };
                Told::Told(maker)
            }
        };
        let device = turn.open(&mut self.gpu)?.device().clone();
        let kept = turn.open(&mut self.kept)?;
        let made = kept.checked_dispatch(&device, &create, by);
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
        if let Told::Told(maker) = told {
            self.owed.dispatches.push((id, maker));
        }
        turn.reply(&Dispatched::Made(DispatchId { id }))?;

        Ok(())
    }
}

/// Whom a dispatch's making leaves the renderer to tell when it has run: its maker, when it runs once, or no one.
enum Told {
    Untold,
    Told(ProtocolRef<(DispatchRan,)>),
}

/// A list of the sender's own placements, the children it calls, each call found one segment beneath the sender, its
/// hits, and the keys it hears, checked whole and kept in place of its last, or refused and logged, the last standing
/// (ADR-0031, ADR-0055). The
/// sender is watched from the first list it sends, as from the first thing it made, so its list goes when it ends.
impl Handles<DrawList> for Renderer {
    type Sends = ();

    fn handle(&mut self, list: DrawList, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let sender = turn.sender();
        let hearers = (turn.cast::<Hearer>(sender), turn.cast::<KeyHearer>(sender));
        let entries: Result<Vec<Listed>, ListRefused> = list
            .entries
            .iter()
            .zip(0..)
            .map(|(entry, at)| {
                let reads =
                    (|child: &PathSegment| turn.child_id(sender, child), |pixels: &Bytes| turn.read(pixels).len());
                let listed = entry.listed(at, hearers, reads);
                listed.map_err(|why| ListRefused { at, why })
            })
            .collect();
        let listed = match entries {
            Ok(entries) => self.kept_for(turn, |kept, by| kept.listed(entries, by))?,
            Err(why) => Err(why),
        };
        if let Err(why) = listed {
            turn.log(Level::Warn, format_args!("a draw list was refused: {why:?}"));
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

/// Releases what the sender made, as it asks. A dispatch that runs once, released before it ran, never runs, and its
/// maker, which released it, is not told.
impl Handles<Release> for Renderer {
    type Sends = (ToAny<(DispatchRan,), DispatchRan>,);

    fn handle(&mut self, release: Release, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let by = turn.sender().id();
        let kept = turn.open(&mut self.kept)?;
        let released = kept.release(release, by);
        let held = kept.reorder_after_release();
        log_held(turn, &held);
        match released {
            Owned::Mine(()) => {}
            Owned::NotOwner => turn.log(Level::Warn, format_args!("{release:?} was not released: another made it")),
            Owned::Unknown => turn.log(Level::Warn, format_args!("{release:?} was not released: no such thing")),
        }
        self.untell_gone(turn)?;

        self.tell_unjoined(turn)
    }
}

impl Renderer {
    /// Tells the maker of each dispatch of `once`, which ran once in a cycle whose commands are submitted, that it ran.
    fn tell_ran<S, I>(&mut self, turn: &mut Turn<'_, Self, S>, once: Vec<u64>) -> Result<(), Fault>
    where
        S: Allows<ToAny<(DispatchRan,), DispatchRan>, I>,
    {
        for ran in once {
            for maker in owed_by(&mut self.owed.dispatches, ran) {
                turn.to(maker).send(&DispatchRan::Ran(DispatchId { id: ran }))?;
            }
        }

        Ok(())
    }

    /// Tells the maker of each dispatch that runs once whose frame is gone for good that it never ran, why beside it,
    /// and takes it out of what is kept.
    fn tell_unjoined<S, I>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<ToAny<(DispatchRan,), DispatchRan>, I>,
    {
        let unjoined = turn.open(&mut self.kept)?.unjoined();
        for (dispatch, why) in unjoined {
            let unrun = DispatchRan::Unrun { dispatch: DispatchId { id: dispatch }, why };
            for maker in owed_by(&mut self.owed.dispatches, dispatch) {
                turn.to(maker).send(&unrun)?;
            }
        }

        Ok(())
    }

    /// Forgets the maker to tell of each dispatch that runs once and went untold: released by its maker, or with all
    /// its maker made as it ended.
    fn untell_gone<S>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault> {
        let kept = turn.open(&mut self.kept)?;
        self.owed.dispatches.retain(|(dispatch, _)| kept.dispatches.get(*dispatch).is_some());

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
    type Sends = (ToAny<(DispatchRan,), DispatchRan>,);

    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.watch.ended(&ended);
        self.timed.ended(&ended);
        let maker = ended.actor().id();
        let kept = turn.open(&mut self.kept)?;
        let watched = kept.makers.ended(maker);
        if !watched {
            return Ok(());
        }
        let held = kept.ended(maker);
        log_held(turn, &held);
        // What the ended actor made to run once went with all it made, and no one is left to tell.
        self.untell_gone(turn)?;

        self.tell_unjoined(turn)
    }
}

impl Kept {
    /// Nothing made yet, on `device`, for the windows `windows`, each drawn by the renderer's own main frame of it,
    /// walked from the program's root list `roots`, with the renderer's own fonts, `faces`, and an empty atlas.
    fn new(device: &wgpu::Device, windows: Vec<Windowed>, roots: Vec<ActorId>, faces: Faces) -> Self {
        let longest = device.limits().max_texture_dimension_2d;
        let picker = Picker::new(device);
        let mut windows_memory = GpuMemory::default();
        for windowed in &windows {
            windows_memory.add(windowed.memory());
        }

        Self {
            ids: Ids::default(),
            makers: Makers::default(),
            meshes: Weighed::default(),
            textures: Weighed::default(),
            programs: Made::default(),
            computes: Made::default(),
            instances: Weighed::default(),
            uniforms: Weighed::default(),
            storage: Weighed::default(),
            placements: Made::default(),
            dispatches: Made::default(),
            lists: Lists::new(roots),
            reads: HashMap::new(),
            samplers: HashMap::new(),
            frames: Weighed::default(),
            order: Vec::new(),
            held: Vec::new(),
            longest,
            most_workgroups: device.limits().max_compute_workgroups_per_dimension,
            windows,
            windows_memory,
            text: Texts::new(device, longest, picker.layout(), faces),
            picker,
        }
    }

    /// Releases what `release` names, when `by` made it, with what goes with it: a frame's texture and its lists, and
    /// the records placements of released instances draw from then.
    fn release(&mut self, release: Release, by: ActorId) -> Owned<()> {
        match release {
            Release::Mesh(mesh) => self.meshes.release(mesh.id, by).map(drop),
            Release::Texture(texture) => self.textures.release(texture.id, by).map(drop),
            Release::Program(program) => self.programs.release(program.id, by).map(drop),
            Release::Compute(compute) => self.computes.release(compute.id, by).map(drop),
            Release::Instances(instances) => {
                let released = self.instances.release(instances.id, by);
                if let Owned::Mine(made) = &released {
                    self.last_drawn(instances.id, made.held.length);
                }
                released.map(drop)
            }
            Release::Uniforms(uniforms) => self.uniforms.release(uniforms.id, by).map(drop),
            Release::Storage(storage) => self.storage.release(storage.id, by).map(drop),
            Release::Placement(placement) => self.release_placement(placement.id, by),
            Release::Dispatch(dispatch) => {
                let released = self.dispatches.release(dispatch.id, by);
                if let Owned::Mine(_) = &released {
                    self.lists.released(dispatch.id, by);
                }
                released.map(drop)
            }
            Release::Font(font) => self.text.faces.made.release(font.id, by).map(drop),
            Release::Text(text) => self.text.release_text(text.id, by, &mut self.lists),
            Release::Frame(frame) => {
                let released = self.frames.release(frame.id, by);
                if let Owned::Mine(walk) = &released {
                    self.unframed([(frame.id, walk.texture())]);
                }
                released.map(drop)
            }
        }
    }

    /// Releases the placement `placement`, of a mesh or of a text, when `by` made it, taking it out of `by`'s lists in
    /// this turn (ADR-0031).
    fn release_placement(&mut self, placement: u64, by: ActorId) -> Owned<()> {
        let released = self.placements.release(placement, by);
        if let Owned::Mine(_) = &released {
            self.lists.released(placement, by);
        }
        let unknown = matches!(released, Owned::Unknown);
        if unknown {
            return self.text.release_placement(placement, by, &mut self.lists);
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
        self.text.ended(maker, &mut self.lists);
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
                Drawing::Indirect { buffer: made.buffer.clone(), at }
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
        let Stages::Draws { reads, picking, .. } = &program.stages else {
            return Err(JoinRefused::Program);
        };
        // A program that reads targets binds them in its first group, which the pass binds.
        let first = u32::from(!reads.is_empty());

        Ok(Joined {
            frame: join.frame,
            pass: join.pass,
            fits: Fits { shape: program.shape, reads: reads.clone() },
            program: join.program.id,
            pipeline: program.pipeline.clone(),
            groups: self.bind_groups(device, &program.groups[first as usize..], &join.bindings, first)?,
            picking: *picking,
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

    /// The dispatch `create`, asked for by `by`, checked and made, with its bind groups made now: why it was refused
    /// when its check refuses it (ADR-0026).
    fn checked_dispatch(
        &mut self,
        device: &wgpu::Device,
        create: &CreateDispatch,
        by: ActorId,
    ) -> Result<Dispatching, DispatchRefused> {
        dispatch::check(create, by, &*self).and_then(|grid| self.dispatch(device, create, grid))
    }

    /// The dispatch `create` asks for, made of what its check found kept, running the grid `grid`, with its bind groups
    /// made now.
    fn dispatch(
        &mut self,
        device: &wgpu::Device,
        create: &CreateDispatch,
        grid: Grid,
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
        let running = match grid {
            Grid::Workgroups(workgroups) => Running::Workgroups(workgroups),
            Grid::Indirect { storage, at } => {
                let made = self.storage.get(storage.id).ok_or(DispatchRefused::Storage)?;
                Running::Indirect { buffer: made.buffer.clone(), at }
            }
        };

        Ok(Dispatching {
            frame: join.frame,
            pass: join.pass,
            runs: create.runs,
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

    /// A bind group for each of `layouts`, the first of the group `first`, binding what `bindings` names in it, kept
    /// with what each binds so a texture made again is bound again.
    fn bind_groups(
        &self,
        device: &wgpu::Device,
        layouts: &[wgpu::BindGroupLayout],
        bindings: &[Bind],
        first: u32,
    ) -> Result<Groups, JoinRefused> {
        let mut entries = Vec::with_capacity(bindings.len());
        for group in (first..).take(layouts.len()) {
            for bind in bindings.iter().filter(|bind| bind.group == group) {
                let missing = JoinRefused::Missing { group, binding: bind.binding };
                let holds = match bind.to {
                    placement::Resource::Uniforms(uniforms) => {
                        Binds::Buffer(self.uniforms.get(uniforms.id).ok_or(missing)?.buffer.clone())
                    }
                    placement::Resource::Storage(storage) => {
                        Binds::Buffer(self.storage.get(storage.id).ok_or(missing)?.buffer.clone())
                    }
                    placement::Resource::Texture(texture) => {
                        let view = self.textures.get(texture.id).ok_or(missing)?.view.clone();
                        Binds::Texture { texture: texture.id, view }
                    }
                    placement::Resource::Sampler(sampling) => {
                        Binds::Sampler(self.samplers.get(&sampling).ok_or(missing)?.clone())
                    }
                };
                entries.push(Entry { group, binding: bind.binding, holds });
            }
        }

        Ok(Groups::new(device, layouts.to_vec(), first, entries))
    }

    /// Records the frame `issued` on `encoder`: each of its passes begun in order, and what it does recorded in it, each
    /// pass writing its timestamps to `stamps` when the frames are timed, each draw as the walk issued it, binding the
    /// picking step through `stepped`. Each draw is counted into `tally`. Answers the ids of the dispatches that run
    /// once that it recorded.
    fn run(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        issued: FrameIssued<'_>,
        stamps: Option<&Stamps>,
        (tally, stepped): (&mut Tally, &wgpu::BindGroup),
    ) -> Vec<u64> {
        let FrameIssued { frame, walk, end, first, steps } = issued;
        let mut once = Vec::new();
        for ((at, (pass, step)), draws) in (first..).zip(walk.steps()).zip(steps) {
            let (into, load, depth) = match step {
                Step::Draw { into, depth, load, .. } => (*into, *load, *depth),
                Step::Effect { effect, .. } => (effect.into, effect.load, DepthUse::Off),
                Step::Compute => {
                    let mut recording = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: None,
                        timestamp_writes: stamps.map(|stamps| stamps.compute_writes(at)),
                    });
                    once.extend(self.compute(&mut recording, frame, pass));
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
                Step::Draw { read, depth, .. } => self.draw(&mut recording, draws, (*depth, read), (tally, stepped)),
                Step::Effect { effect, .. } => {
                    recording.set_pipeline(&effect.pipeline);
                    recording.set_bind_group(0, &effect.read.group, &[]);
                    for (group, bound) in effect.groups.each() {
                        recording.set_bind_group(group, bound, &[]);
                    }
                    recording.draw(0..3, 0..1);
                    tally.draw(1);
                }
                Step::Compute => {}
            }
        }

        once
    }

    /// Drops what the renderer draws to `window` with, which has closed: its own main frame of it, and the frame an
    /// actor made in its place, with what each made for it, as no window's id is given again. Answers whether it drew
    /// to the window.
    fn closed(&mut self, window: WindowId) -> bool {
        let Some(at) = self.windows.iter().position(|windowed| windowed.window == window) else {
            return false;
        };
        let closed = self.windows.remove(at);
        self.windows_memory.remove(closed.memory());
        if let Main::Made(id) = closed.main {
            self.frames.forget(id);
        }

        true
    }

    /// Draws to the window `windowed` from now, opened after those it draws to.
    fn opened(&mut self, windowed: Windowed) {
        self.windows_memory.add(windowed.memory());
        self.windows.push(windowed);
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

    /// Keeps each frame of `frames`, just drawn onto its window, as the window's last, on `encoder`.
    fn keep_last(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        frames: &[(WindowId, wgpu::Texture)],
    ) {
        for (window, frame) in frames {
            let windowed = self.windows.iter_mut().find(|windowed| windowed.window == *window);
            if let Some(windowed) = windowed {
                reweighed(&mut self.windows_memory, windowed, |windowed| windowed.last.keep(device, encoder, frame));
            }
        }
    }

    /// The main frame of `window`: none when the renderer does not draw to it.
    fn main(&self, window: WindowId) -> Option<&Walk> {
        self.windowed(window).map(|windowed| self.main_of(windowed))
    }

    /// The frame `frame` names: a window's main frame, or one kept under its id. None when there is none.
    fn walk(&self, frame: Frame) -> Option<&Walk> {
        match frame {
            Frame::Main(window) => self.main(window),
            Frame::Texture(id) => self.frames.get(id.id),
        }
    }

    /// What the copy of `region`, in the end's pixels, of the target at `target` of `frame` reads: the target's kind,
    /// the region in its pixels, and its texture. Refused when there is no such frame or target, when the target holds
    /// four samples of depth, and when none of the region is within the end.
    fn copyable(
        &self,
        frame: Frame,
        target: u32,
        region: Region,
    ) -> Result<(TargetKind, Region, wgpu::Texture), ReadRefused> {
        let walk = self.walk(frame).ok_or(ReadRefused::Frame)?;
        let (spec, texture) = walk.copied(target).ok_or(ReadRefused::Target)?;
        let is_multisampled = spec.samples == Samples::Four && spec.kind == TargetKind::Depth;
        if is_multisampled {
            return Err(ReadRefused::Multisampled);
        }
        let scaled = region.scaled(walk.size(), spec.scale).ok_or(ReadRefused::Outside)?;

        Ok((spec.kind, scaled, texture.clone()))
    }

    /// Fits the main frame of `window` to its window, drawn at `size` this `Render`, and each frame that follows it:
    /// none is fitted when the renderer does not draw to it.
    fn fit(&mut self, device: &wgpu::Device, window: WindowId, size: Size) {
        let Some(windowed) = self.windows.iter_mut().find(|windowed| windowed.window == window) else {
            return;
        };
        let made = match windowed.main {
            Main::Own => None,
            Main::Made(id) => self.frames.change(id, |walk| walk.fit(device, size)),
        };
        if made.is_none() {
            reweighed(&mut self.windows_memory, windowed, |windowed| windowed.own.fit(device, size));
        }
        self.follow(device, window, size);
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
    /// the renderer's own again when it was one of them, and the id of each texture is taken back, as the frames and
    /// placements that read it keep it as it was last drawn (ADR-0025).
    fn unframed(&mut self, released: impl IntoIterator<Item = (u64, Option<u64>)>) {
        for (id, texture) in released {
            for windowed in &mut self.windows {
                if windowed.main == Main::Made(id) {
                    windowed.main = Main::Own;
                }
            }
            if let Some(texture) = texture {
                self.textures.forget(texture);
            }
        }
    }

    /// Checks `list`, sent by `by`, whole, and keeps it in place of `by`'s last list: refused as [`Lists::check`]
    /// refuses it.
    fn listed(&mut self, list: Vec<Listed>, by: ActorId) -> Result<(), ListRefused> {
        let listable = Listable { meshes: &self.placements, texts: &self.text.placed, dispatches: &self.dispatches };

        self.lists.check(list, by, &listable)
    }

    /// The roots the walk of the lists for `frame` begins from: the program's root list for a window's main frame, and
    /// the frame's maker for one that ends on a texture, whose list is that frame's root list (ADR-0055).
    fn roots(&self, frame: Frame) -> Vec<ActorId> {
        match frame {
            Frame::Main(_) => self.lists.roots().to_vec(),
            Frame::Texture(texture) => self.frames.maker(texture.id).into_iter().collect(),
        }
    }

    /// The `Keys` entries the walk of `window`'s main frame holds, the last called first, as its actor sends each key
    /// through them (ADR-0055).
    fn keys_of(&self, window: WindowId) -> Vec<KeysEntry> {
        let frame = Frame::Main(window);
        let mut keys: Vec<KeysEntry> = self
            .lists
            .walk(self.roots(frame))
            .filter_map(|walking| match walking.entry {
                Listed::Keys { frame: declared, entry } if *declared == frame => Some(entry.clone()),
                Listed::Keys { .. }
                | Listed::Draw { .. }
                | Listed::Run(_)
                | Listed::Call { .. }
                | Listed::Hit { .. } => None,
            })
            .collect();
        keys.reverse();

        keys
    }

    /// The `Keys` entries of each of `windows`, those a `Render` draws, whose walk holds others than its actor was last
    /// sent: kept as sent, each beside its window, to be sent to its actor.
    fn relisted(&mut self, windows: impl IntoIterator<Item = WindowId>) -> Vec<(WindowId, KeysListed)> {
        let mut relisted = Vec::new();
        for window in windows {
            let keys = self.keys_of(window);
            let Some(windowed) = self.windows.iter_mut().find(|windowed| windowed.window == window) else {
                continue;
            };
            let changed = windowed.keys != keys;
            if changed {
                windowed.keys.clone_from(&keys);
                relisted.push((window, KeysListed { entries: keys }));
            }
        }

        relisted
    }

    /// Who owns the main frame of `window`: the maker of the frame that is it, none while it is the renderer's own or
    /// the renderer does not draw to the window.
    fn owner(&self, window: WindowId) -> Option<ActorId> {
        let Main::Made(id) = self.windowed(window)?.main else {
            return None;
        };

        self.frames.maker(id)
    }

    /// The end of the frame `id`, made for `maker`: a texture of `size` and `format`, kept as an ordinary texture.
    fn end_of(&mut self, device: &wgpu::Device, id: u64, size: Size, format: TextureFormat, maker: ActorId) -> End {
        let texture = self.ids.next();
        let (made, view) = end_texture(device, size, format);
        let origin = Origin::Frame(id);
        self.textures.keep(texture, maker, Texture { made, view: view.clone(), format, origin });

        End::Texture { texture, view }
    }

    /// Fits each frame that follows `window` to it, drawn at `size` this `Render`: one whose size that makes is
    /// another has its texture made again under its id, its targets made again, and the texture bound again wherever
    /// it was bound (ADR-0049).
    fn follow(&mut self, device: &wgpu::Device, window: WindowId, size: Size) {
        let resized: Vec<(u64, Size)> = self
            .frames
            .iter()
            .filter_map(|(id, walk)| {
                let (width, height) = walk.follows(window)?;
                let followed = size.divided(width, height);
                (followed != walk.size()).then_some((id, followed))
            })
            .collect();
        for (id, size) in resized {
            let Some(texture) = self.frames.get(id).and_then(Walk::texture) else {
                continue;
            };
            let Some(format) = self.textures.get(texture).map(|kept| kept.format) else {
                continue;
            };
            let (made, view) = end_texture(device, size, format);
            self.textures.change(texture, |kept| {
                kept.made = made;
                kept.view = view.clone();
            });
            self.frames.change(id, |walk| walk.refit(device, size, view.clone()));
            self.rebind(device, texture, &view);
        }
    }

    /// Binds `view` wherever the texture `texture` was bound: by placements, dispatches, and effects.
    fn rebind(&mut self, device: &wgpu::Device, texture: u64, view: &wgpu::TextureView) {
        let joins = self.placements.values_mut().flat_map(|placement| &mut placement.joins);
        for join in joins {
            join.groups.rebind(device, texture, view);
        }
        for dispatch in self.dispatches.values_mut() {
            dispatch.groups.rebind(device, texture, view);
        }
        self.frames.change_each(|walk| walk.rebind(device, texture, view));
    }

    /// Checks that `by` may make a frame that ends as `ends` says: only the owner of a window's main frame replaces
    /// it.
    fn may_make(&self, ends: Ends, by: ActorId) -> Result<(), FrameRefused> {
        match ends {
            Ends::Window(window) => frame::may_replace(self.owner(window), by),
            Ends::Texture { .. } | Ends::Follows { .. } => Ok(()),
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
            Ends::Texture { size, format } => (self.end_of(device, id, size, format, maker), format.colour(), size),
            // Made at one pixel and fitted at the first `Render` that draws its window, as a main frame is.
            Ends::Follows { format, .. } => {
                (self.end_of(device, id, Size::PIXEL, format, maker), format.colour(), Size::PIXEL)
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

    /// Every frame a `Render` that draws into `windows` runs, in the order it runs them: those that end on textures in
    /// their order, then the main frame of each window with a frame among them, in the order of the windows.
    fn ran<'a>(&'a self, windows: &'a [(WindowId, wgpu::TextureView)]) -> impl Iterator<Item = (Frame, &'a Walk)> + 'a {
        let textures =
            self.order.iter().filter_map(|id| Some((Frame::Texture(FrameId { id: *id }), self.frames.get(*id)?)));
        let mains = self.shown(windows).map(|(window, main)| (Frame::Main(window), main));

        textures.chain(mains)
    }

    /// The picking of a `Render` that draws into `windows`: each window among them probed where its actor last said
    /// the pointer was, pressed, and released, its probes written on `queue`, and its places pinned taken (ADR-0055).
    fn picking(&mut self, queue: &wgpu::Queue, windows: &[(WindowId, wgpu::TextureView)]) -> Picking {
        let probed = self
            .windows
            .iter_mut()
            .filter(|windowed| windows.iter().any(|(window, _)| *window == windowed.window))
            .filter_map(|windowed| windowed.probing.take(queue, windowed.window))
            .collect();

        self.picker.picking(probed)
    }

    /// How many passes a `Render` runs that draws into `windows`: every pass of each frame it runs.
    fn passes(&self, windows: &[(WindowId, wgpu::TextureView)]) -> u32 {
        self.ran(windows).map(|(_, walk)| walk.passes()).sum()
    }

    /// Each pass a `Render` that draws into `windows` runs, beside its frame, in the order it runs them, which is the
    /// order they write their timestamps in (ADR-0053).
    fn pass_names(&self, windows: &[(WindowId, wgpu::TextureView)]) -> Vec<Ran> {
        self.ran(windows)
            .flat_map(|(frame, walk)| {
                walk.specs().iter().map(move |spec| Ran { frame, pass: spec.name, label: spec.label.clone() })
            })
            .collect()
    }

    /// Records every frame on `encoder` in the order it runs them, a frame that ends on a window drawing into that
    /// window's frame among `windows`, each pass writing its timestamps to `stamps` in the order they run when the
    /// frames are timed, and each draw counted into `tally` (ADR-0025, ADR-0053, ADR-0055). Every frame is walked
    /// first, each call recorded for picking, and the calls' buffer fitted on `gpu` to every record and written, before
    /// any draw is recorded, so each draw binds a record the buffer holds. Answers the ids of the dispatches that run
    /// once that it recorded, which run once these commands are submitted, each probed window's stacks copied on
    /// `encoder` to read back, and how many draws were refused, past the most calls a buffer holds.
    fn run_all(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        gpu: (&wgpu::Device, &wgpu::Queue),
        windows: &[(WindowId, wgpu::TextureView)],
        stamps: Option<&Stamps>,
        tally: &mut Tally,
    ) -> (Vec<u64>, Vec<Stacks>, usize) {
        let (device, queue) = gpu;
        let mut picking = self.picking(queue, windows);
        let issued = self.issue_all(windows, &mut picking);
        let recorded = self.picker.bound(gpu, picking);
        let mut once = Vec::new();
        for frame in issued {
            let stepped = recorded.group(frame.frame);
            once.extend(self.run(encoder, frame, stamps, (tally, stepped)));
        }

        let past = recorded.past();
        let (calls, probed) = recorded.finish();
        self.picker.keep(calls);
        let stacks = probed.into_iter().map(|framed| framed.copied(device, encoder)).collect();

        (once, stacks, past)
    }

    /// Every frame a `Render` that draws into `windows` runs, in the order it runs them, walked and issued before any
    /// of it is recorded, each call recorded into `picking`: a frame that ends on a window with none among them is not
    /// issued, though its passes keep their places among the `Render`'s.
    fn issue_all<'a>(
        &'a self,
        windows: &'a [(WindowId, wgpu::TextureView)],
        picking: &mut Picking,
    ) -> Vec<FrameIssued<'a>> {
        let mut first = 0;
        let mut issued = Vec::new();
        for (frame, walk) in self.ran(windows) {
            let at = first;
            first += walk.passes();
            let Some(end) = walk.end(windows) else {
                continue;
            };
            picking.enter(frame);
            let mut drawn = 0;
            let mut steps = Vec::new();
            for (pass, step) in walk.steps() {
                let Step::Draw { into, depth, fits, .. } = step else {
                    steps.push(Vec::new());
                    continue;
                };
                let attachment = match into {
                    Target::End => end.texture(),
                    Target::Texture(target) => walk.target(*target).drawn().texture(),
                };
                let in_pass = InPass {
                    at: drawn,
                    tests_depth: *depth != DepthUse::Off,
                    scale: walk.scale_of(*into),
                    size: [attachment.width(), attachment.height()],
                    per_pixel: walk.per_pixel(),
                };
                drawn += 1;
                steps.push(self.issue((frame, pass), fits, (picking, in_pass)));
            }
            issued.push(FrameIssued { frame, walk, end, first: at, steps });
        }

        issued
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

    /// Records into `recording` every dispatch run in the compute pass `pass` of `frame`, in the order
    /// [`computed`](Self::computed) answers, setting a pipeline only where the dispatch run before used another compute
    /// program. Answers the ids of those that run once that it recorded.
    fn compute(&self, recording: &mut wgpu::ComputePass<'_>, frame: Frame, pass: PassName) -> Vec<u64> {
        let joined = self.computed(frame, pass);
        let once = joined.iter().filter(|(_, dispatch)| dispatch.runs == Runs::Once).map(|(id, _)| *id).collect();
        let mut pipeline = 0;
        for (_, dispatch) in joined {
            // Ids begin at one, so no compute program's is the zero it begins from.
            if pipeline != dispatch.compute {
                recording.set_pipeline(&dispatch.pipeline);
                pipeline = dispatch.compute;
            }
            for (group, bound) in dispatch.groups.each() {
                recording.set_bind_group(group, bound, &[]);
            }
            match &dispatch.running {
                Running::Workgroups([x, y, z]) => recording.dispatch_workgroups(*x, *y, *z),
                Running::Indirect { buffer, at } => recording.dispatch_workgroups_indirect(buffer, *at),
            }
        }

        once
    }

    /// Each dispatch run in the compute pass `pass` of `frame`, under its id, in the order it runs: as the lists walked
    /// from the frame's roots name them, each in its list's order, each child's where its parent calls it, whether it
    /// runs until released or once (ADR-0055). One no list walked names is not run, and neither is one held, as it
    /// writes what another writes in a frame it is in no order with.
    fn computed(&self, frame: Frame, pass: PassName) -> Vec<(u64, &Dispatching)> {
        self.lists
            .walk(self.roots(frame))
            .filter_map(|walking| match *walking.entry {
                Listed::Run(id) => self.dispatches.get(id).map(|dispatch| (id, dispatch)),
                Listed::Draw { .. } | Listed::Call { .. } | Listed::Hit { .. } | Listed::Keys { .. } => None,
            })
            .filter(|(id, dispatch)| {
                let joins = dispatch.runs_in(frame, pass);
                let held = self.held.contains(id);
                joins && !held
            })
            .collect()
    }

    /// Releases each dispatch of `once`, which runs once and ran in a cycle whose commands are submitted, taking it
    /// out of its maker's list, and orders the frames again without it.
    fn ran_once(&mut self, once: &[u64]) {
        for dispatch in once {
            self.dispatched_once(*dispatch);
        }
        self.reorder();
    }

    /// Releases the dispatch `dispatch`, which ran once or never will, taking it out of its maker's list as a release
    /// by its maker does.
    fn dispatched_once(&mut self, dispatch: u64) {
        if let Some(maker) = self.dispatches.maker(dispatch) {
            self.dispatches.forget(dispatch);
            self.lists.released(dispatch, maker);
        }
    }

    /// Releases each dispatch that runs once whose frame is gone for good, and answers each beside why it never ran:
    /// the window whose main frame it joined closed, or the frame that ends on a texture it joined was released. A
    /// window's main frame replaced by one without its pass is not gone: the dispatch waits for a frame with it.
    fn unjoined(&mut self) -> Vec<(u64, Unrun)> {
        let unjoined: Vec<(u64, Unrun)> = self
            .dispatches
            .iter()
            .filter(|(_, dispatch)| dispatch.runs == Runs::Once)
            .filter_map(|(id, dispatch)| {
                let why = match dispatch.frame {
                    Frame::Main(window) => self.windowed(window).is_none().then_some(Unrun::Closed),
                    Frame::Texture(frame) => self.frames.get(frame.id).is_none().then_some(Unrun::Released),
                };
                why.map(|why| (id, why))
            })
            .collect();
        if unjoined.is_empty() {
            return unjoined;
        }
        for (dispatch, _) in &unjoined {
            self.dispatched_once(*dispatch);
        }
        self.reorder();

        unjoined
    }

    /// Each draw of the pass `pass` of `frame`, whose programs are made against `fits`, as the walk issues it, before
    /// any is recorded, in the order [`drawn`](Self::drawn) answers: each draw cut to its list's clip, scaled to the
    /// pass's target as `in_pass` says, a draw cut to nothing not issued, and given its call's offset, which moves it
    /// to its list's origin, its call and each hit numbered into `picking` and keyed by the pass (ADR-0055). A draw
    /// past the most calls a buffer holds is refused, counted by `picking`, and not issued.
    fn issue(
        &self,
        (frame, pass): (Frame, PassName),
        fits: &Fits,
        (picking, in_pass): (&mut Picking, InPass),
    ) -> Vec<IssuedDraw<'_>> {
        let mut draws = Vec::new();
        for issued in self.drawn((frame, pass), fits) {
            let (drawable, picks, within) = match issued {
                Issued::Draw { drawable, picks, within } => (drawable, picks, within),
                Issued::Hit { rect, picks, origin } => {
                    picking.hit(rect, (picks, origin), in_pass);
                    continue;
                }
            };
            let Cut::Pixels { x, y, width, height } = within.clip.pixels(in_pass.scale, in_pass.size) else {
                continue;
            };
            let Ok(offset) = picking.call(picks, within.origin, in_pass) else {
                continue;
            };
            draws.push(IssuedDraw { drawable, scissor: [x, y, width, height], offset });
        }

        draws
    }

    /// Records into `recording` each draw of a pass as the walk issued it, `draws`, in a pass that uses depth as `depth`
    /// says, setting a pipeline only where the placement drawn before used another program, with the targets the pass
    /// reads bound at group 0 through `read` when it reads any. A placed text is drawn with the glyph program's
    /// pipeline for the pass. Each draw is cut to its scissor, counted into `tally`, and binds the picking step through
    /// `stepped` at its call's offset, which moves it to its list's origin (ADR-0055).
    fn draw(
        &self,
        recording: &mut wgpu::RenderPass<'_>,
        draws: Vec<IssuedDraw<'_>>,
        (depth, read): (DepthUse, &ReadGroup),
        (tally, stepped): (&mut Tally, &wgpu::BindGroup),
    ) {
        // The targets it reads are bound once, for every program that joins it, which lists nothing in group 0.
        let reads = !read.reads.is_empty();
        if reads {
            recording.set_bind_group(0, &read.group, &[]);
        }
        let mut pipeline = 0;
        for IssuedDraw { drawable, scissor: [x, y, width, height], offset } in draws {
            recording.set_scissor_rect(x, y, width, height);
            let (placement, join) = match drawable {
                Drawable::Mesh(placement, join) => (placement, join),
                Drawable::Text(placed, join) => {
                    self.text.draw(recording, (placed, join), depth, tally, (stepped, offset));
                    // The next mesh drawn sets its own pipeline, as no program's id is zero.
                    pipeline = 0;
                    continue;
                }
            };
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
            for (group, bound) in join.groups.each() {
                recording.set_bind_group(group, bound, &[]);
            }
            recording.set_bind_group(join.picking, stepped, &[offset]);
            recording.set_vertex_buffer(0, placement.vertices.slice(..));
            if let Drawing::Records { buffer, .. } = &placement.drawing {
                recording.set_vertex_buffer(1, buffer.slice(..));
            }
            recording.set_index_buffer(placement.indices.slice(..), wgpu::IndexFormat::Uint32);
            match count {
                Count::Given(count) => {
                    recording.draw_indexed(0..placement.index_count, 0, 0..count);
                    tally.draw(stats::triangles(placement.index_count, count));
                }
                Count::Held { buffer, at } => {
                    recording.draw_indexed_indirect(buffer, at);
                    tally.draw(0);
                }
            }
        }
    }

    /// Each placement drawn in the pass `pass` of `frame`, whose programs are made against `fits`, with its join of the
    /// pass, in the order it is drawn: the lists walked from the frame's roots, each in its order, each child drawn
    /// where its parent calls it, where its parent's call places it, meshes and texts alike, with each hit listed in
    /// the pass in its place (ADR-0031, ADR-0052, ADR-0055). A join of another shape, or reading other targets, is not
    /// drawn, and neither is a placement no list walked holds.
    fn drawn(&self, (frame, pass): (Frame, PassName), fits: &Fits) -> Vec<Issued<'_>> {
        self.lists.walk(self.roots(frame)).filter_map(|walking| self.issued(walking, (frame, pass), fits)).collect()
    }

    /// What the entry `walking` reaches issues in the pass `pass` of `frame`, whose programs are made against `fits`:
    /// its placement's join drawn there, where its list is, or itself, a hit listed there, its rect moved to its
    /// list's origin and cut to its list's clip; none when it neither draws nor is hit there.
    fn issued<'a>(&'a self, walking: Walking<'a>, (frame, pass): (Frame, PassName), fits: &Fits) -> Option<Issued<'a>> {
        let Walking { entry, within } = walking;
        match entry {
            Listed::Draw { placement, picks } => {
                let drawable = self.listed_in(*placement, (frame, pass), fits)?;
                Some(Issued::Draw { drawable, picks, within })
            }
            Listed::Hit { frame: hit, pass: hit_pass, rect, picks } => {
                let is_here = *hit == frame && *hit_pass == pass;
                is_here.then_some(Issued::Hit { rect: within.cut(*rect), picks, origin: within.origin })
            }
            Listed::Run(_) | Listed::Call { .. } | Listed::Keys { .. } => None,
        }
    }

    /// The placement `placement`, of a mesh or of a text, with its join drawn in the pass `pass` of `frame`, whose
    /// programs are made against `fits`: none when it does not join the pass, or is of another shape.
    /// The placement is kept, as a release takes it out of its maker's list in the release's turn.
    fn listed_in(&self, placement: u64, (frame, pass): (Frame, PassName), fits: &Fits) -> Option<Drawable<'_>> {
        if let Some(made) = self.placements.get(placement) {
            let join = made.joins.iter().find(|join| join.draws_in(frame, pass, fits))?;
            return Some(Drawable::Mesh(made, join));
        }
        let placed = self.text.placed.get(placement)?;
        let join = placed.joins().iter().find(|join| join.draws_in(frame, pass, fits))?;

        Some(Drawable::Text(placed, join))
    }
}

/// What a pass issues in its order, each as the pointer reaches it: a placement's join drawn where its list is, or a
/// hit, which draws nothing, over its rect in the window's pixels, its list's origin at `origin` (ADR-0055).
enum Issued<'a> {
    Draw { drawable: Drawable<'a>, picks: &'a Picks, within: Within },
    Hit { rect: Rect2, picks: &'a Picks, origin: Vec2 },
}

/// One draw of a pass as the walk issued it, before it is recorded: what it draws, the pixels of its pass's target it is
/// cut to, `[x, y, width, height]`, and the offset of its call, whose record moves it to its list's origin.
struct IssuedDraw<'a> {
    drawable: Drawable<'a>,
    scissor: [u32; 4],
    offset: u32,
}

/// A frame of a `Render` as the walk issued it, before any of it is recorded: the frame, its walk, the view it ends
/// on, the place of its first pass among the `Render`'s, and the draws of each of its passes, in order, none for a
/// pass that draws nothing.
struct FrameIssued<'a> {
    frame: Frame,
    walk: &'a Walk,
    end: &'a wgpu::TextureView,
    first: u32,
    steps: Vec<Vec<IssuedDraw<'a>>>,
}

/// One placement's join drawn in a pass: a mesh's, or a text's.
enum Drawable<'a> {
    Mesh(&'a Placement, &'a Joined),
    Text(&'a PlacedText, &'a TextJoined),
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

/// What a list's check finds: the placements of meshes and of texts, under one count of ids, and the dispatches, those
/// that run until released and those that run once alike.
struct Listable<'a> {
    meshes: &'a Made<Placement>,
    texts: &'a Made<PlacedText>,
    dispatches: &'a Made<Dispatching>,
}

impl Listables for Listable<'_> {
    fn placement_maker(&self, placement: u64) -> Option<ActorId> {
        self.meshes.maker(placement).or_else(|| self.texts.maker(placement))
    }

    fn dispatch_maker(&self, dispatch: u64) -> Option<ActorId> {
        self.dispatches.maker(dispatch)
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
            Self::Pixels { buffer, .. }
            | Self::Target { buffer, .. }
            | Self::Bytes { buffer }
            | Self::Stamps { buffer, .. }
            | Self::Picks(Stacks { buffer, .. }) => buffer,
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

/// The copy of `region` of `texture`, a frame's target holding what `kind` says, into a buffer the chassis can map,
/// recorded on `encoder`, with where the region lies in what was copied. WebGPU copies only the whole of a depth
/// texture, so of depth the whole is copied, its depth alone, and the region cut from it once mapped; of any other,
/// the region alone.
fn copy_region(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    region: Region,
    kind: TargetKind,
) -> Reading {
    let (copy, within) = match kind {
        TargetKind::Depth => {
            let whole = Region { x: 0, y: 0, width: texture.width(), height: texture.height() };
            (whole, region)
        }
        TargetKind::Float | TargetKind::Uint => (region, Region { x: 0, y: 0, ..region }),
    };
    let copied = Copied::new(copy.width, copy.height, readback::target_bytes(kind));
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: copied.bytes(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let layout =
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(copied.row), rows_per_image: Some(copied.height) };
    let aspect = match kind {
        TargetKind::Depth => wgpu::TextureAspect::DepthOnly,
        TargetKind::Float | TargetKind::Uint => wgpu::TextureAspect::All,
    };
    let origin = wgpu::Origin3d { x: copy.x, y: copy.y, z: 0 };
    let size = wgpu::Extent3d { width: copy.width, height: copy.height, depth_or_array_layers: 1 };
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin, aspect },
        wgpu::TexelCopyBufferInfo { buffer: &buffer, layout },
        size,
    );

    Reading::Target { buffer, copied, within, region, kind }
}

/// The copy of `storage`, of `size` bytes, into a buffer the chassis can map, recorded on `encoder`.
fn copy_storage(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    storage: &wgpu::Buffer,
    size: u64,
) -> Reading {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_buffer_to_buffer(storage, 0, &buffer, 0, size);

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
    /// passes, each tick `period` nanoseconds, named for subscribers as `named` says.
    fn resolve(self, encoder: &mut wgpu::CommandEncoder, period: f32, named: Named) -> Reading {
        let bytes = u64::from(self.count) * u64::from(wgpu::QUERY_SIZE);
        encoder.resolve_query_set(&self.set, 0..self.count, &self.resolved, 0);
        encoder.copy_buffer_to_buffer(&self.resolved, 0, &self.read, 0, bytes);

        Reading::Stamps { buffer: self.read, period, named }
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
        let dispatched = self.dispatches.iter().map(|(_, dispatch)| (dispatch.frame, dispatch.textures.as_slice()));
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
        let Stages::Draws { layout, instances, reads, .. } = &program.stages else {
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

    fn storage(&self, id: StorageId) -> Option<u64> {
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
        let dispatched = self.dispatches.iter().flat_map(|(_, dispatch)| {
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

/// A texture a frame ends on, `size` and of `format`, to draw into, bind, and copy from, and its view.
fn end_texture(device: &wgpu::Device, size: Size, format: TextureFormat) -> (wgpu::Texture, wgpu::TextureView) {
    let made = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: size.width, height: size.height, depth_or_array_layers: 1 },
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

    (made, view)
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
    /// Makes storage of `size` bytes from the buffer `buffer` makes, once they are checked, answering with it or why
    /// it was refused.
    fn store(
        &mut self,
        turn: &mut Turn<'_, Self, (ToSender<Stored>,)>,
        size: u64,
        buffer: impl FnOnce(&Gpu) -> wgpu::Buffer,
    ) -> Result<(), Fault> {
        let most = turn.open(&mut self.gpu)?.device().limits().max_storage_buffer_binding_size;
        let size = match storage::check(size, most) {
            Ok(size) => size,
            Err(why) => {
                turn.reply(&Stored::Refused(why))?;
                return Ok(());
            }
        };

        let buffer = buffer(turn.open(&mut self.gpu)?);
        let made = Storage { buffer, size };
        let id = self.made(turn, |kept, id, maker| kept.storage.keep(id, maker, made))?;
        turn.reply(&Stored::Made(StorageId { id }))?;

        Ok(())
    }

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

    /// Sends each of `windows`, those a `Render` drew, the `Keys` entries its walk holds, when they have changed since
    /// it was last sent them (ADR-0055).
    fn relist<S, I>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        windows: impl IntoIterator<Item = WindowId>,
    ) -> Result<(), Fault>
    where
        S: Allows<Tells<Path<Window>, KeysListed>, I>,
    {
        let relisted = turn.open(&mut self.kept)?.relisted(windows);
        for (window, listed) in relisted {
            turn.to(path::<Window>(window)).send(&listed)?;
        }

        Ok(())
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

/// What storage is used for besides being bound: copied from as it is read back, and read as a draw's or a dispatch's
/// arguments by any that asks to.
const STORAGE_USAGE: wgpu::BufferUsages =
    wgpu::BufferUsages::STORAGE.union(wgpu::BufferUsages::INDIRECT).union(wgpu::BufferUsages::COPY_SRC);

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

/// The pipeline `create` asks for, of `wgsl`, the WGSL its blob holds with the picking step added, drawing into passes
/// of its shape, binding the step's group through `picking` after its own, made from what its check let through.
fn pipeline(gpu: &Gpu, create: &CreateProgram, wgsl: &str, picking: &wgpu::BindGroupLayout) -> Program {
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
    let bound: Vec<Option<&wgpu::BindGroupLayout>> = groups.iter().chain([picking]).map(Some).collect();
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
            picking: program::picking_group(create),
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
