//! The page's entry points, and an actor that makes items in storage, makes the window's main frame with three compute
//! passes before the one that draws, dispatches into them the three passes of an ordered compaction that culls the
//! items into a list and a count each frame, and places a program drawn as many times as the count says, then reads
//! the count and the list back to see that the GPU kept what it expects, in order.

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, On, Path, Render, Start, Tick, Top,
    Turn, path,
};
use aethereal_chassis_browser::{Browser, Started};
use aethereal_data::{Bound, Plain};
use aethereal_render::Renderer;
use aethereal_render::compute::{ComputeId, Computed, CreateCompute};
use aethereal_render::dispatch::{CreateDispatch, Dispatch, Dispatched, Invocations};
use aethereal_render::frame::builder::{FrameBuilder, Onto, depth};
use aethereal_render::frame::{CreateFrame, Ends, Frame, Framed, ShapeOf, Shaped};
use aethereal_render::mesh::{Attribute, CreateMesh, MeshId, Meshed, VertexFormat, VertexLayout};
use aethereal_render::pass::{Main, Pass};
use aethereal_render::placement::{Bind, Drawn, Join, Place, Placed, Resource};
use aethereal_render::program::{
    Access, Binding, BindingKind, Blend, Coverage, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId,
    Programmed,
};
use aethereal_render::readback::{Bytes, ReadStorage};
use aethereal_render::shaders::WORKGROUP_SIZE;
use aethereal_render::shape::Shape;
use aethereal_render::storage::{CreateStorage, StorageId, Stored};
use aethereal_render::timing::{TimeFrame, Timed};
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Unbuilt};
use aethereal_window::{InitialWindows, WindowId, Windows};
use core::cell::RefCell;
use core::mem;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::wasm_bindgen;

thread_local! {
    /// What the culler was answered, for the page to show.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Notes what the culler was answered.
fn say(line: String) {
    SAID.with_borrow_mut(|said| said.push(line));
}

/// The program the page runs, for its `requestAnimationFrame` to call.
#[wasm_bindgen]
pub struct Program {
    started: Started,
}

/// Bootstraps the program on the page with the window root, which opens the page's canvas as the one window,
/// the renderer, which opens WebGPU on it, and the culler.
///
/// # Errors
///
/// When the canvas or WebGPU on it will not open, the page will not be listened to, or a root does not begin.
#[wasm_bindgen(wasm_bindgen_futures = aethereal_chassis_browser::wasm_bindgen_futures)]
pub async fn start() -> Result<Program, JsError> {
    let started = Browser::program()
        .with_cycle::<(Tick, Render)>()
        // Its blobs hold a frame of a large canvas.
        .with_engine(EngineSettings { blob_bytes: 1 << 26, ..Default::default() })
        .root::<Windows>()
        .root::<Renderer>()
        .root::<Culler>()
        .run()?
        .await?;

    Ok(Program { started })
}

#[wasm_bindgen]
impl Program {
    /// Runs one frame, at the timestamp `requestAnimationFrame` handed its callback, in milliseconds.
    pub fn frame(&mut self, timestamp_millis: f64) {
        self.started.frame(timestamp_millis);
    }

    /// What the culler was answered since this was last asked, a line each.
    #[must_use]
    #[expect(clippy::unused_self, reason = "the page asks it of the program it started")]
    pub fn said(&self) -> Vec<String> {
        SAID.with_borrow_mut(mem::take)
    }
}

/// The culler draws on the one window the window root opened.
impl<H: Host> Bootstrap<H> for Culler {
    const SECTION: &'static str = "culler";
    type Settings = ();
    type Needs = (InitialWindows,);
    type Provides = ();

    async fn bootstrap(
        (): (),
        (windows,): (InitialWindows,),
        cx: &mut Bootstrapping<'_, H, Self>,
    ) -> Result<(), Unbuilt> {
        cx.begin(&(), Shows { window: windows.first() })
    }
}

/// The context of an ask that needs none.
#[derive(Plain)]
#[kind(name = "aether.example.nothing")]
struct Nothing;

/// The culler's config: the window it draws on.
#[derive(Plain)]
#[kind(name = "aether.example.shows")]
struct Shows {
    window: WindowId,
}

/// What was learned before the quad was asked for: the shape the program draws into.
#[derive(Plain)]
#[kind(name = "aether.example.learned")]
struct Learned {
    shape: Shape,
}

/// The compaction's first pass: each workgroup counts the items inside the disc it keeps.
#[derive(Plain)]
#[kind(name = "aether.example.tally")]
struct Tally;

impl Pass for Tally {}

/// The compaction's second pass: one workgroup scans the counts into where each workgroup's kept items begin, and
/// writes how many were kept into the draw's arguments.
#[derive(Plain)]
#[kind(name = "aether.example.offsets")]
struct Offsets;

impl Pass for Offsets {}

/// The compaction's third pass: each kept item's index is written at its place in the list.
#[derive(Plain)]
#[kind(name = "aether.example.scatter")]
struct Scatter;

impl Pass for Scatter {}

/// The cull, `cull.wesl`, linked to WGSL by the build script with the shared `scan` and `compact` pieces: an entry
/// point for each of the compaction's three passes.
const CULL: &str = include_str!(concat!(env!("OUT_DIR"), "/indirect_cull.wgsl"));

/// A program that draws the quad, small, at the item the visible list names for its instance, coloured by the item's
/// place among all of them.
const DOTS: &str = "
@group(0) @binding(0) var<storage, read> items: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read> visible: array<u32>;

struct Between {
    @builtin(position) at: vec4<f32>,
    @location(0) colour: vec4<f32>,
}

@vertex
fn place(@location(0) corner: vec2<f32>, @builtin(instance_index) at: u32) -> Between {
    let item = visible[at];
    let hue = f32(item) / f32(arrayLength(&items));
    return Between(vec4(corner * 0.02 + items[item], 0.0, 1.0), vec4(hue, 0.7, 1.0 - hue, 1.0));
}

@fragment
fn shade(between: Between) -> @location(0) vec4<f32> {
    return between.colour;
}
";

/// The items along each side of the grid they lie on.
const SIDE: u32 = 32;

/// The items: each a place of two floats, eight bytes. They span four of the compaction's workgroups.
const ITEMS: u32 = SIDE * SIDE;

/// The workgroups of the first and third passes, one for each [`WORKGROUP_SIZE`] items: the words of the totals.
const GROUPS: u32 = ITEMS.div_ceil(WORKGROUP_SIZE);

/// The square of the disc's radius, 0.75: an item is kept when the square of its distance from the middle is less.
const RADIUS_SQUARED: f32 = 0.5625;

/// The quad's indices, the first word of the arguments.
const QUAD_INDICES: u32 = 6;

/// The frames drawn before the arguments and the list are read back.
const SETTLED: u32 = 3;

/// The culler: it makes what it runs and draws one ask after another, each answer carrying what was made so far, makes
/// the frame, places and dispatches into it, and reads the arguments and the list back once a few frames have passed.
#[derive(Bound)]
#[kind(name = "aether.example.culler")]
enum Culler {
    /// Its asks are under way, and nothing runs on `window`.
    Making { window: WindowId },
    /// The dispatches run and the placement draws, counted from `arguments`, the list `visible`: `drawn` frames have
    /// passed since.
    Running { arguments: StorageId, visible: StorageId, drawn: u32 },
    /// It has asked for the arguments and the list.
    Reading,
}

impl ChildOf for Culler {
    type Parent = Top;
}

/// The bytes of `values`, as a blob is put.
fn bytes(values: impl IntoIterator<Item = u32>) -> Vec<u8> {
    values.into_iter().flat_map(u32::to_ne_bytes).collect()
}

/// The bytes of `values`, floats, as a blob is put.
fn floats(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    bytes(values.into_iter().map(f32::to_bits))
}

/// The place of each item: the middles of a grid of [`SIDE`] by [`SIDE`] cells over the canvas, each a whole number of
/// thirty-seconds, so its square and the cull's test of it are exact on the CPU and the GPU alike.
fn places() -> impl Iterator<Item = [f32; 2]> {
    (0..ITEMS).map(|at| {
        let cell = |along: u32| (f32::from(u16::try_from(along).unwrap_or(0)) + 0.5) / 16.0 - 1.0;
        [cell(at % SIDE), cell(at / SIDE)]
    })
}

/// The index of each item inside the disc, in the items' order, as the compaction lists them.
fn expected() -> Vec<u32> {
    (0..).zip(places()).filter(|(_, [x, y])| x * x + y * y < RADIUS_SQUARED).map(|(at, _)| at).collect()
}

/// The words of `bytes`, as storage is read back.
fn words(bytes: &[u8]) -> Vec<u32> {
    bytes.chunks_exact(4).filter_map(|word| word.try_into().ok()).map(u32::from_ne_bytes).collect()
}

/// The three compute programs of the compaction, one for each of its passes.
#[derive(Plain)]
#[kind(name = "aether.example.culls")]
struct Culls {
    tally: ComputeId,
    offsets: ComputeId,
    scatter: ComputeId,
}

/// What was made before the compute programs were asked for.
#[derive(Plain)]
#[kind(name = "aether.example.drawing")]
struct Drawing {
    mesh: MeshId,
    dots: ProgramId,
}

/// Which compute program was asked for, and what was made before it.
#[derive(Plain)]
#[kind(name = "aether.example.computing")]
enum Computing {
    Tally { drawing: Drawing },
    Offsets { drawing: Drawing, tally: ComputeId },
    Scatter { drawing: Drawing, tally: ComputeId, offsets: ComputeId },
}

/// What was made before the storage was asked for.
#[derive(Plain)]
#[kind(name = "aether.example.made_programs")]
struct MadePrograms {
    mesh: MeshId,
    dots: ProgramId,
    culls: Culls,
}

/// Which storage was asked for, and what was made before it: the items, the totals, the visible list, then the
/// arguments.
#[derive(Plain)]
#[kind(name = "aether.example.storing")]
enum Storing {
    Items { made: MadePrograms },
    Totals { made: MadePrograms, items: StorageId },
    Visible { made: MadePrograms, items: StorageId, totals: StorageId },
    Arguments { made: MadePrograms, items: StorageId, totals: StorageId, visible: StorageId },
}

/// Everything made before the frame was asked for, which the placement and the dispatches are made with.
#[derive(Plain)]
#[kind(name = "aether.example.made")]
struct Made {
    mesh: MeshId,
    dots: ProgramId,
    culls: Culls,
    items: StorageId,
    totals: StorageId,
    visible: StorageId,
    arguments: StorageId,
}

/// Which pass's dispatch was asked for, with everything made.
#[derive(Plain)]
#[kind(name = "aether.example.dispatching")]
enum Dispatching {
    Tally { made: Made },
    Offsets { made: Made },
    Scatter { made: Made },
}

/// Which storage a reading is of.
#[derive(Debug, Plain)]
#[kind(name = "aether.example.read")]
enum Read {
    Arguments,
    Visible,
}

impl Actor for Culler {
    type Config = Shows;
    type Key = ();
    type Handles = (
        On<Start>,
        On<Tick>,
        Answer<Shaped, Nothing>,
        Answer<Meshed, Learned>,
        Answer<Programmed, MeshId>,
        Answer<Computed, Computing>,
        Answer<Stored, Storing>,
        Answer<Framed, Made>,
        Answer<Placed, Made>,
        Answer<Dispatched, Dispatching>,
        Answer<Bytes, Read>,
        Answer<Timed, Nothing>,
    );
    type BeginSends = ();
    type EndSends = ();

    fn begin(shows: Shows, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();
        turn.subscribe::<Tick>();

        Ok(Self::Making { window: shows.window })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Start>> for Culler {
    type Sends = (Asks<Path<Renderer>, ShapeOf, Nothing>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Self::Making { window } = *self else {
            return Ok(());
        };
        turn.to(path::<Renderer>(())).ask(&ShapeOf::pass::<Main>(Frame::Main(window)), &Nothing)?;

        Ok(())
    }
}

impl Handles<Answer<Shaped, Nothing>> for Culler {
    type Sends = (Asks<Path<Renderer>, CreateMesh, Learned>,);

    fn handle(
        &mut self,
        (answered, _): (Answered<Shaped>, Nothing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Shaped::Shape(shape)) = answered else {
            say(format!("the window's main pass has no shape: {answered:?}"));
            return Ok(());
        };
        let quad = floats([-1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0]);
        let (vertices, indices) = match (turn.put(quad), turn.put(bytes([0, 1, 2, 0, 2, 3]))) {
            (Ok(vertices), Ok(indices)) => (vertices, indices),
            (Err(why), _) | (_, Err(why)) => {
                say(format!("nothing asked: {why}"));
                return Ok(());
            }
        };
        let corner = Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 };
        let layout = VertexLayout { stride: 8, attributes: vec![corner] };
        turn.to(path::<Renderer>(())).ask(&CreateMesh { layout, vertices, indices }, &Learned { shape })?;

        Ok(())
    }
}

/// Storage bound at `binding` as `access` says, of which the program binds at least `bytes`.
const fn stored(binding: u32, bytes: u32, access: Access) -> Binding {
    Binding { group: 0, binding, kind: BindingKind::Storage { bytes, access } }
}

impl Handles<Answer<Meshed, Learned>> for Culler {
    type Sends = (Asks<Path<Renderer>, CreateProgram, MeshId>,);

    fn handle(
        &mut self,
        (answered, learned): (Answered<Meshed>, Learned),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Meshed::Made(mesh)) = answered else {
            say(format!("the quad was not made: {answered:?}"));
            return Ok(());
        };
        let wgsl = match turn.put(Vec::from(DOTS)) {
            Ok(wgsl) => wgsl,
            Err(why) => {
                say(format!("no dots asked: {why}"));
                return Ok(());
            }
        };
        let corner = Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 };
        // Nothing per instance: the shader reads the item its instance names from storage.
        let dots = CreateProgram {
            wgsl,
            vertex: String::from("place"),
            fragment: String::from("shade"),
            layout: VertexLayout { stride: 8, attributes: vec![corner] },
            instances: PerInstance::None,
            bindings: vec![stored(0, 8, Access::Read), stored(1, 4, Access::Read)],
            reads: Vec::new(),
            state: Fixed { blend: Blend::Opaque, depth: Depth::Off, cull: Cull::None, coverage: Coverage::Off },
            shape: learned.shape,
        };
        turn.to(path::<Renderer>(())).ask(&dots, &mesh)?;

        Ok(())
    }
}

/// The compute program of the cull's entry point `entry`, binding what that pass binds: the items at 0, the totals at
/// 1, the visible list at 2, the arguments at 3, and the totals again at 4, to read the offsets.
fn cull(entry: &str, turn: &mut Turn<'_, Culler, impl Sized>) -> Result<CreateCompute, String> {
    let bindings = match entry {
        "tally" => vec![stored(0, 8, Access::Read), stored(1, 4, Access::Write)],
        "offsets" => vec![stored(1, 4, Access::Write), stored(3, 20, Access::Write)],
        _ => vec![stored(0, 8, Access::Read), stored(2, 4, Access::Write), stored(4, 4, Access::Read)],
    };
    let wgsl = turn.put(Vec::from(CULL)).map_err(|why| format!("no {entry} asked: {why}"))?;

    Ok(CreateCompute { wgsl, entry: String::from(entry), bindings })
}

impl Handles<Answer<Programmed, MeshId>> for Culler {
    type Sends = (Asks<Path<Renderer>, CreateCompute, Computing>,);

    fn handle(
        &mut self,
        (answered, mesh): (Answered<Programmed>, MeshId),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Programmed::Made(dots)) = answered else {
            say(format!("the dots were not made: {answered:?}"));
            return Ok(());
        };
        let tally = match cull("tally", turn) {
            Ok(tally) => tally,
            Err(why) => {
                say(why);
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&tally, &Computing::Tally { drawing: Drawing { mesh, dots } })?;

        Ok(())
    }
}

impl Handles<Answer<Computed, Computing>> for Culler {
    type Sends = (Asks<Path<Renderer>, CreateCompute, Computing>, Asks<Path<Renderer>, CreateStorage, Storing>);

    fn handle(
        &mut self,
        (answered, computing): (Answered<Computed>, Computing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Computed::Made(made)) = answered else {
            say(format!("a compute program was not made: {answered:?}"));
            return Ok(());
        };
        let (entry, next) = match computing {
            Computing::Tally { drawing } => ("offsets", Computing::Offsets { drawing, tally: made }),
            Computing::Offsets { drawing, tally } => ("scatter", Computing::Scatter { drawing, tally, offsets: made }),
            Computing::Scatter { drawing, tally, offsets } => {
                let culls = Culls { tally, offsets, scatter: made };
                let made = MadePrograms { mesh: drawing.mesh, dots: drawing.dots, culls };
                let bytes = match turn.put(floats(places().flatten())) {
                    Ok(bytes) => bytes,
                    Err(why) => {
                        say(format!("no items asked: {why}"));
                        return Ok(());
                    }
                };
                turn.to(path::<Renderer>(())).ask(&CreateStorage { bytes }, &Storing::Items { made })?;
                return Ok(());
            }
        };
        let create = match cull(entry, turn) {
            Ok(create) => create,
            Err(why) => {
                say(why);
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&create, &next)?;

        Ok(())
    }
}

impl Handles<Answer<Stored, Storing>> for Culler {
    type Sends = (Asks<Path<Renderer>, CreateStorage, Storing>, Asks<Path<Renderer>, CreateFrame, Made>);

    fn handle(
        &mut self,
        (answered, storing): (Answered<Stored>, Storing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Stored::Made(storage)) = answered else {
            say(format!("storage was not made: {answered:?}"));
            return Ok(());
        };
        let Self::Making { window } = *self else {
            return Ok(());
        };
        let (bytes, next) = match storing {
            // A word for each workgroup of the first and third passes.
            Storing::Items { made } => (bytes((0..GROUPS).map(|_| 0)), Storing::Totals { made, items: storage }),
            // Room for every item, as many as the cull could keep.
            Storing::Totals { made, items } => {
                (bytes((0..ITEMS).map(|_| 0)), Storing::Visible { made, items, totals: storage })
            }
            // The quad's indices, and a count the second pass writes at every `Render`.
            Storing::Visible { made, items, totals } => {
                (bytes([QUAD_INDICES, 0, 0, 0, 0]), Storing::Arguments { made, items, totals, visible: storage })
            }
            Storing::Arguments { made: programs, items, totals, visible } => {
                let made = Made {
                    mesh: programs.mesh,
                    dots: programs.dots,
                    culls: programs.culls,
                    items,
                    totals,
                    visible,
                    arguments: storage,
                };
                // The window's main frame: the compaction's three passes, each run after the one before, then the one
                // that draws what they kept, onto the window with depth, as the renderer's own `Main` is.
                let mut frame = FrameBuilder::on(Ends::Window(window));
                frame.compute(Tally);
                frame.compute(Offsets);
                frame.compute(Scatter);
                let create = match frame.finish(Main, Onto(depth::Write, ())) {
                    Ok(create) => create,
                    Err(why) => {
                        say(format!("the frame was not built: {why:?}"));
                        return Ok(());
                    }
                };
                turn.to(path::<Renderer>(())).ask(&create, &made)?;
                return Ok(());
            }
        };
        let bytes = match turn.put(bytes) {
            Ok(bytes) => bytes,
            Err(why) => {
                say(format!("no storage asked: {why}"));
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&CreateStorage { bytes }, &next)?;

        Ok(())
    }
}

/// `storage` bound at each of `bindings`, in group 0.
fn bound(bindings: &[(u32, StorageId)]) -> Vec<Bind> {
    bindings
        .iter()
        .map(|(binding, storage)| Bind { group: 0, binding: *binding, to: Resource::Storage(*storage) })
        .collect()
}

impl Handles<Answer<Framed, Made>> for Culler {
    type Sends = (Asks<Path<Renderer>, Place, Made>,);

    fn handle(
        &mut self,
        (answered, made): (Answered<Framed>, Made),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Framed::Made(_)) = answered else {
            say(format!("the frame was not made: {answered:?}"));
            return Ok(());
        };
        let Self::Making { window } = *self else {
            return Ok(());
        };
        let bindings = bound(&[(0, made.items), (1, made.visible)]);
        let place = Place {
            mesh: made.mesh,
            instances: Drawn::Indirect { storage: made.arguments, at: 0 },
            joins: vec![Join::to::<Main>(Frame::Main(window), made.dots, bindings)],
        };
        turn.to(path::<Renderer>(())).ask(&place, &made)?;

        Ok(())
    }
}

/// The dispatch of the compaction's pass `dispatching` names, into the main frame of `window`: the first and third
/// one invocation for each item, the second one workgroup.
fn dispatch(window: WindowId, dispatching: &Dispatching) -> CreateDispatch {
    let frame = Frame::Main(window);
    match dispatching {
        Dispatching::Tally { made } => CreateDispatch {
            join: Dispatch::to::<Tally>(frame, made.culls.tally, bound(&[(0, made.items), (1, made.totals)])),
            size: Invocations::PerElement(made.items),
        },
        Dispatching::Offsets { made } => CreateDispatch {
            join: Dispatch::to::<Offsets>(frame, made.culls.offsets, bound(&[(1, made.totals), (3, made.arguments)])),
            size: Invocations::Workgroups([1, 1, 1]),
        },
        Dispatching::Scatter { made } => {
            let bindings = bound(&[(0, made.items), (2, made.visible), (4, made.totals)]);
            CreateDispatch {
                join: Dispatch::to::<Scatter>(frame, made.culls.scatter, bindings),
                size: Invocations::PerElement(made.items),
            }
        }
    }
}

impl Handles<Answer<Placed, Made>> for Culler {
    type Sends = (Asks<Path<Renderer>, CreateDispatch, Dispatching>,);

    fn handle(
        &mut self,
        (answered, made): (Answered<Placed>, Made),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Placed::Made(_)) = answered else {
            say(format!("the dots were not placed: {answered:?}"));
            return Ok(());
        };
        let Self::Making { window } = *self else {
            return Ok(());
        };
        let dispatching = Dispatching::Tally { made };
        turn.to(path::<Renderer>(())).ask(&dispatch(window, &dispatching), &dispatching)?;

        Ok(())
    }
}

impl Handles<Answer<Dispatched, Dispatching>> for Culler {
    type Sends = (Asks<Path<Renderer>, CreateDispatch, Dispatching>,);

    fn handle(
        &mut self,
        (answered, dispatching): (Answered<Dispatched>, Dispatching),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Dispatched::Made(_)) = answered else {
            say(format!("a pass of the cull was not dispatched: {answered:?}"));
            return Ok(());
        };
        let Self::Making { window } = *self else {
            return Ok(());
        };
        let next = match dispatching {
            Dispatching::Tally { made } => Dispatching::Offsets { made },
            Dispatching::Offsets { made } => Dispatching::Scatter { made },
            Dispatching::Scatter { made } => {
                say(format!("placed dispatched items={ITEMS} workgroups={GROUPS}"));
                *self = Self::Running { arguments: made.arguments, visible: made.visible, drawn: 0 };
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&dispatch(window, &next), &next)?;

        Ok(())
    }
}

impl Handles<On<Tick>> for Culler {
    type Sends = (Asks<Path<Renderer>, ReadStorage, Read>, Asks<Path<Renderer>, TimeFrame, Nothing>);

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Self::Running { arguments, visible, drawn } = *self else {
            return Ok(());
        };
        if drawn < SETTLED {
            *self = Self::Running { arguments, visible, drawn: drawn + 1 };
            return Ok(());
        }
        turn.to(path::<Renderer>(()))
            .ask(&ReadStorage { storage: arguments }, &Read::Arguments)?
            .ask(&ReadStorage { storage: visible }, &Read::Visible)?
            .ask(&TimeFrame, &Nothing)?;
        *self = Self::Reading;

        Ok(())
    }
}

impl Handles<Answer<Bytes, Read>> for Culler {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, read): (Answered<Bytes>, Read),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Bytes::Read { bytes }) = answered else {
            say(format!("{read:?} was not read: {answered:?}"));
            return Ok(());
        };
        let words = words(turn.read(&bytes));
        let expected = expected();
        let said = match read {
            Read::Arguments => {
                let (Some(indices), Some(visible)) = (words.first(), words.get(1)) else {
                    say(format!("the arguments were short: words={}", words.len()));
                    return Ok(());
                };
                let wanted = expected.len();
                let same = usize::try_from(*visible).is_ok_and(|visible| visible == wanted);
                format!("read indices={indices} visible={visible} expected={wanted} of={ITEMS} same={same}")
            }
            Read::Visible => {
                let in_order = words.get(..expected.len()).is_some_and(|kept| kept == expected);
                format!("read listed={} in_order={in_order}", expected.len())
            }
        };
        say(said);

        Ok(())
    }
}

impl Handles<Answer<Timed, Nothing>> for Culler {
    type Sends = ();

    fn handle(&mut self, (answered, _): (Answered<Timed>, Nothing), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let said = match answered {
            Answered::Yes(Timed::Took { passes }) => format!("timed passes_nanos={passes:?}"),
            Answered::Yes(Timed::Refused(why)) => format!("refused timing={why:?}"),
            Answered::No(why) => format!("unanswered timing={why:?}"),
        };
        say(said);

        Ok(())
    }
}
