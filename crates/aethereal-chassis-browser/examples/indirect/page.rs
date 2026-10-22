//! The page's entry points, and an actor that makes items in storage, dispatches a compute program that culls them
//! into a list and a count each frame, and places a program drawn as many times as the count says, then reads the
//! count back to see that the GPU counted what it expects.

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, Level, On, Path, Render, Start, Tells,
    Tick, Top, Turn, path,
};
use aethereal_chassis_browser::{Browser, Started};
use aethereal_data::{Bound, Plain};
use aethereal_render::Renderer;
use aethereal_render::compute::{ComputeId, Computed, CreateCompute};
use aethereal_render::dispatch::{CreateDispatch, Dispatch, Dispatched, Invocations};
use aethereal_render::frame::{Frame, ShapeOf, Shaped};
use aethereal_render::mesh::{Attribute, CreateMesh, MeshId, Meshed, VertexFormat, VertexLayout};
use aethereal_render::pass::{Compute, Main};
use aethereal_render::placement::{Bind, Drawn, Join, Place, Placed, Resource};
use aethereal_render::program::{
    Access, Binding, BindingKind, Blend, Coverage, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId,
    Programmed,
};
use aethereal_render::readback::{Bytes, ReadStorage};
use aethereal_render::shape::Shape;
use aethereal_render::storage::{CreateStorage, StorageId, Stored, WriteStorage};
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
        .map_err(|unordered| JsError::new(&format!("the cycle is out of order: {unordered:?}")))?
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

/// A compute program that keeps each item inside the disc: it counts it into the arguments' instance count, and
/// writes its index where the count stood. The last workgroup's invocations past the items' end stop.
const CULL: &str = "
struct Arguments {
    index_count: u32,
    instance_count: atomic<u32>,
    first_index: u32,
    base_vertex: u32,
    first_instance: u32,
}

@group(0) @binding(0) var<storage, read> items: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> visible: array<u32>;
@group(0) @binding(2) var<storage, read_write> arguments: Arguments;

@compute @workgroup_size(64)
fn cull(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= arrayLength(&items) {
        return;
    }
    let at = items[id.x];
    if dot(at, at) >= 0.5625 {
        return;
    }
    let slot = atomicAdd(&arguments.instance_count, 1u);
    visible[slot] = id.x;
}
";

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

/// The items: each a place of two floats, eight bytes.
const ITEMS: u32 = SIDE * SIDE;

/// The square of the disc's radius, 0.75: an item is kept when the square of its distance from the middle is less.
const RADIUS_SQUARED: f32 = 0.5625;

/// The quad's indices, the first word of the arguments.
const QUAD_INDICES: u32 = 6;

/// The frames drawn before the arguments are read back.
const SETTLED: u32 = 3;

/// The culler: it makes what it runs and draws one ask after another, each answer carrying what was made so far,
/// places and dispatches it, resets the count at each `Tick`, and reads the arguments back once a few frames have
/// passed.
#[derive(Bound)]
#[kind(name = "aether.example.culler")]
enum Culler {
    /// Its asks are under way, and nothing runs on `window`.
    Making { window: WindowId },
    /// The dispatch runs and the placement draws, counted from `arguments`: `drawn` frames have passed since.
    Running { arguments: StorageId, drawn: u32 },
    /// It has asked for the arguments.
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

/// How many items lie inside the disc, as the cull counts them.
fn expected() -> u32 {
    let inside = places().filter(|[x, y]| x * x + y * y < RADIUS_SQUARED).count();

    u32::try_from(inside).unwrap_or(u32::MAX)
}

/// The arguments as each frame begins: the quad's indices, a count of nothing, and the rest from zero.
fn reset() -> Vec<u8> {
    bytes([QUAD_INDICES, 0, 0, 0, 0])
}

/// What was made before the compute program was asked for.
#[derive(Plain)]
#[kind(name = "aether.example.drawing")]
struct Drawing {
    mesh: MeshId,
    dots: ProgramId,
}

/// What was made before the items were asked for.
#[derive(Plain)]
#[kind(name = "aether.example.made_programs")]
struct MadePrograms {
    mesh: MeshId,
    dots: ProgramId,
    cull: ComputeId,
}

/// What a storage was asked for after: the items after the programs, the visible list after the items, and the
/// arguments after the visible list.
#[derive(Plain)]
#[kind(name = "aether.example.storing")]
enum Storing {
    Items(MadePrograms),
    Visible(MadeItems),
    Arguments(MadeVisible),
}

/// What was made before the visible list was asked for.
#[derive(Plain)]
#[kind(name = "aether.example.made_items")]
struct MadeItems {
    made: MadePrograms,
    items: StorageId,
}

/// What was made before the arguments were asked for.
#[derive(Plain)]
#[kind(name = "aether.example.made_visible")]
struct MadeVisible {
    made: MadeItems,
    visible: StorageId,
}

/// What the placement was asked for with: what is dispatched once it is made.
#[derive(Plain)]
#[kind(name = "aether.example.placing")]
struct Placing {
    cull: ComputeId,
    items: StorageId,
    visible: StorageId,
    arguments: StorageId,
}

/// What the dispatch was asked for with: the arguments it counts into.
#[derive(Plain)]
#[kind(name = "aether.example.dispatching")]
struct Dispatching {
    arguments: StorageId,
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
        Answer<Computed, Drawing>,
        Answer<Stored, Storing>,
        Answer<Placed, Placing>,
        Answer<Dispatched, Dispatching>,
        Answer<Bytes, Nothing>,
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

impl Handles<Answer<Programmed, MeshId>> for Culler {
    type Sends = (Asks<Path<Renderer>, CreateCompute, Drawing>,);

    fn handle(
        &mut self,
        (answered, mesh): (Answered<Programmed>, MeshId),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Programmed::Made(dots)) = answered else {
            say(format!("the dots were not made: {answered:?}"));
            return Ok(());
        };
        let wgsl = match turn.put(Vec::from(CULL)) {
            Ok(wgsl) => wgsl,
            Err(why) => {
                say(format!("no culling asked: {why}"));
                return Ok(());
            }
        };
        let cull = CreateCompute {
            wgsl,
            entry: String::from("cull"),
            bindings: vec![stored(0, 8, Access::Read), stored(1, 4, Access::Write), stored(2, 20, Access::Write)],
        };
        turn.to(path::<Renderer>(())).ask(&cull, &Drawing { mesh, dots })?;

        Ok(())
    }
}

impl Handles<Answer<Computed, Drawing>> for Culler {
    type Sends = (Asks<Path<Renderer>, CreateStorage, Storing>,);

    fn handle(
        &mut self,
        (answered, drawing): (Answered<Computed>, Drawing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Computed::Made(cull)) = answered else {
            say(format!("the compute program was not made: {answered:?}"));
            return Ok(());
        };
        let bytes = match turn.put(floats(places().flatten())) {
            Ok(bytes) => bytes,
            Err(why) => {
                say(format!("no items asked: {why}"));
                return Ok(());
            }
        };
        let made = MadePrograms { mesh: drawing.mesh, dots: drawing.dots, cull };
        turn.to(path::<Renderer>(())).ask(&CreateStorage { bytes }, &Storing::Items(made))?;

        Ok(())
    }
}

/// The items bound first and the visible list second, as both programs bind them.
fn listed(items: StorageId, visible: StorageId) -> Vec<Bind> {
    vec![
        Bind { group: 0, binding: 0, to: Resource::Storage(items) },
        Bind { group: 0, binding: 1, to: Resource::Storage(visible) },
    ]
}

impl Handles<Answer<Stored, Storing>> for Culler {
    type Sends = (Asks<Path<Renderer>, CreateStorage, Storing>, Asks<Path<Renderer>, Place, Placing>);

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
            // Room for every item, as many as the cull could keep.
            Storing::Items(made) => {
                (bytes((0..ITEMS).map(|_| 0)), Storing::Visible(MadeItems { made, items: storage }))
            }
            Storing::Visible(made) => (reset(), Storing::Arguments(MadeVisible { made, visible: storage })),
            Storing::Arguments(made) => {
                let (programs, items, visible) = (made.made.made, made.made.items, made.visible);
                let place = Place {
                    mesh: programs.mesh,
                    instances: Drawn::Indirect { storage, at: 0 },
                    joins: vec![Join::to::<Main>(Frame::Main(window), programs.dots, listed(items, visible))],
                };
                let placing = Placing { cull: programs.cull, items, visible, arguments: storage };
                turn.to(path::<Renderer>(())).ask(&place, &placing)?;
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

impl Handles<Answer<Placed, Placing>> for Culler {
    type Sends = (Asks<Path<Renderer>, CreateDispatch, Dispatching>,);

    fn handle(
        &mut self,
        (answered, placing): (Answered<Placed>, Placing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Placed::Made(_)) = answered else {
            say(format!("the dots were not placed: {answered:?}"));
            return Ok(());
        };
        let Self::Making { window } = *self else {
            return Ok(());
        };
        let mut bindings = listed(placing.items, placing.visible);
        bindings.push(Bind { group: 0, binding: 2, to: Resource::Storage(placing.arguments) });
        // Run in the compute pass before the one the dots are drawn in, so each frame draws what it counted.
        let dispatch = CreateDispatch {
            join: Dispatch::to::<Compute>(Frame::Main(window), placing.cull, bindings),
            size: Invocations::PerElement(placing.items),
        };
        turn.to(path::<Renderer>(())).ask(&dispatch, &Dispatching { arguments: placing.arguments })?;

        Ok(())
    }
}

impl Handles<Answer<Dispatched, Dispatching>> for Culler {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, dispatching): (Answered<Dispatched>, Dispatching),
        _: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Dispatched::Made(_)) = answered else {
            say(format!("the cull was not dispatched: {answered:?}"));
            return Ok(());
        };
        say(format!("placed dispatched items={ITEMS}"));
        *self = Self::Running { arguments: dispatching.arguments, drawn: 0 };

        Ok(())
    }
}

impl Handles<On<Tick>> for Culler {
    type Sends = (
        Tells<Path<Renderer>, WriteStorage>,
        Asks<Path<Renderer>, ReadStorage, Nothing>,
        Asks<Path<Renderer>, TimeFrame, Nothing>,
    );

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Self::Running { arguments, drawn } = *self else {
            return Ok(());
        };
        let Ok(bytes) = turn.put(reset()) else {
            turn.log(Level::Warn, "the count was not reset: its bytes were not put");
            return Ok(());
        };
        let mut renderer = turn.to(path::<Renderer>(()));
        if drawn < SETTLED {
            renderer.send(&WriteStorage { storage: arguments, first: 0, bytes })?;
            *self = Self::Running { arguments, drawn: drawn + 1 };
            return Ok(());
        }
        // Read before the reset, so the copy holds what the last `Render` counted: a write lands before what is
        // submitted after it is handled, and the read's copy is submitted as it is handled.
        renderer
            .ask(&ReadStorage { storage: arguments }, &Nothing)?
            .ask(&TimeFrame, &Nothing)?
            .send(&WriteStorage { storage: arguments, first: 0, bytes })?;
        *self = Self::Reading;

        Ok(())
    }
}

impl Handles<Answer<Bytes, Nothing>> for Culler {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, _): (Answered<Bytes>, Nothing),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Bytes::Read { bytes }) = answered else {
            say(format!("the arguments were not read: {answered:?}"));
            return Ok(());
        };
        let words: Vec<u32> =
            turn.read(&bytes).chunks_exact(4).filter_map(|word| word.try_into().ok()).map(u32::from_ne_bytes).collect();
        let (Some(indices), Some(visible)) = (words.first(), words.get(1)) else {
            say(format!("the arguments were short: words={}", words.len()));
            return Ok(());
        };
        let expected = expected();
        let same = *visible == expected;
        say(format!("read indices={indices} visible={visible} expected={expected} of={ITEMS} same={same}"));

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
