//! The page's entry points, and an actor that seeds a field of particles, dispatches a compute program that moves
//! them and places a program that draws them, then reads the field back twice to see that it moved.

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, On, Path, Render, Start, Tick, Top,
    Turn, path,
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
use aethereal_render::storage::{CreateStorage, StorageId, Stored};
use aethereal_render::timing::{TimeFrame, Timed};
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Unbuilt};
use aethereal_window::{InitialWindows, WindowId, Windows};
use core::cell::RefCell;
use core::mem;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::wasm_bindgen;

thread_local! {
    /// What the simulator was answered, for the page to show.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Notes what the simulator was answered.
fn say(line: String) {
    SAID.with_borrow_mut(|said| said.push(line));
}

/// The program the page runs, for its `requestAnimationFrame` to call.
#[wasm_bindgen]
pub struct Program {
    started: Started,
}

/// Bootstraps the program on the page with the window root, which opens the page's canvas as the one window,
/// the renderer, which opens WebGPU on it, and the simulator.
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
        .root::<Simulator>()
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

    /// What the simulator was answered since this was last asked, a line each.
    #[must_use]
    #[expect(clippy::unused_self, reason = "the page asks it of the program it started")]
    pub fn said(&self) -> Vec<String> {
        SAID.with_borrow_mut(mem::take)
    }
}

/// The simulator draws on the one window the window root opened.
impl<H: Host> Bootstrap<H> for Simulator {
    const SECTION: &'static str = "simulator";
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

/// The simulator's config: the window it draws on.
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

/// A compute program that moves each particle of the field by its speed, turning it back at the canvas's edges. The
/// last workgroup's invocations past the field's end stop.
const SIMULATE: &str = "
struct Particle { at: vec2<f32>, speed: vec2<f32> }

@group(0) @binding(0) var<storage, read_write> particles: array<Particle>;

@compute @workgroup_size(64)
fn simulate(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= arrayLength(&particles) {
        return;
    }
    var particle = particles[id.x];
    particle.at += particle.speed;
    let past = abs(particle.at) > vec2(0.95);
    particle.speed = select(particle.speed, -particle.speed, past);
    particle.at = clamp(particle.at, vec2(-0.95), vec2(0.95));
    particles[id.x] = particle;
}
";

/// A program that draws the quad, small, at each particle it reads from the field, by its instance, coloured by its
/// place in the field.
const DOTS: &str = "
struct Particle { at: vec2<f32>, speed: vec2<f32> }

@group(0) @binding(0) var<storage, read> particles: array<Particle>;

struct Between {
    @builtin(position) at: vec4<f32>,
    @location(0) colour: vec4<f32>,
}

@vertex
fn place(@location(0) corner: vec2<f32>, @builtin(instance_index) at: u32) -> Between {
    let hue = f32(at) / f32(arrayLength(&particles));
    return Between(vec4(corner * 0.015 + particles[at].at, 0.0, 1.0), vec4(hue, 0.7, 1.0 - hue, 1.0));
}

@fragment
fn shade(between: Between) -> @location(0) vec4<f32> {
    return between.colour;
}
";

/// The particles of the field: each a place and a speed of two floats, sixteen bytes.
const PARTICLES: u32 = 256;

/// The frames drawn before the field is first read back, and between the two reads.
const SETTLED: u32 = 3;

/// The simulator: it makes what it runs and draws one ask after another, each answer carrying what was made so far,
/// dispatches and places it, then reads the field back twice, a few frames apart.
#[derive(Bound)]
#[kind(name = "aether.example.simulator")]
enum Simulator {
    /// Its asks are under way, and nothing runs on `window`.
    Making { window: WindowId },
    /// The dispatch runs and the placement draws: `drawn` frames have passed since.
    Running { storage: StorageId, drawn: u32 },
    /// It has asked for `storage`, the field, the first time.
    Reading { storage: StorageId },
    /// It read the field as `first` holds it, and `drawn` frames have passed since.
    Read { storage: StorageId, first: Digest, drawn: u32 },
    /// It has asked for the field again.
    Asked { first: Digest },
}

impl ChildOf for Simulator {
    type Parent = Top;
}

/// What is kept of a reading of the field: a digest of its bytes, and the first particle's place across, its bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.example.digest")]
struct Digest {
    bytes: u64,
    x: u32,
}

impl Digest {
    /// The digest of `bytes`, the field as read: FNV-1a over its bytes.
    fn of(bytes: &[u8]) -> Self {
        let digest = bytes
            .iter()
            .fold(0xcbf2_9ce4_8422_2325_u64, |digest, byte| (digest ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3));
        let x = bytes.get(..4).and_then(|x| x.try_into().ok()).map_or(0, u32::from_ne_bytes);

        Self { bytes: digest, x }
    }

    /// The first particle's place across.
    fn x(self) -> f32 {
        f32::from_bits(self.x)
    }
}

/// What was made before the compute program was asked for.
#[derive(Plain)]
#[kind(name = "aether.example.drawing")]
struct Drawing {
    mesh: MeshId,
    dots: ProgramId,
}

/// What was made before the field was asked for.
#[derive(Plain)]
#[kind(name = "aether.example.made_programs")]
struct MadePrograms {
    mesh: MeshId,
    dots: ProgramId,
    simulate: ComputeId,
}

/// What the placement was asked for with: what is dispatched once it is made.
#[derive(Plain)]
#[kind(name = "aether.example.placing")]
struct Placing {
    simulate: ComputeId,
    storage: StorageId,
}

/// What the dispatch was asked for with: the field it moves.
#[derive(Plain)]
#[kind(name = "aether.example.dispatching")]
struct Dispatching {
    storage: StorageId,
}

impl Actor for Simulator {
    type Config = Shows;
    type Key = ();
    type Handles = (
        On<Start>,
        On<Tick>,
        Answer<Shaped, Nothing>,
        Answer<Meshed, Learned>,
        Answer<Programmed, MeshId>,
        Answer<Computed, Drawing>,
        Answer<Stored, MadePrograms>,
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

/// The bytes of `values`, as a blob is put.
fn bytes(values: impl IntoIterator<Item = u32>) -> Vec<u8> {
    values.into_iter().flat_map(u32::to_ne_bytes).collect()
}

/// The bytes of `values`, floats, as a blob is put.
fn floats(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    bytes(values.into_iter().map(f32::to_bits))
}

/// The field as it is seeded: the particles on a grid over the middle of the canvas, each with a speed of its own,
/// from a fixed sequence of whole numbers.
fn seeded() -> Vec<u8> {
    let mut next = 0x2545_f491_u32;
    let mut speed = move || {
        next = next.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let fraction = f32::from(u16::try_from(next >> 16).unwrap_or(0)) / f32::from(u16::MAX);
        (fraction - 0.5) * 0.02
    };
    let side = 16;
    let field = (0..PARTICLES).flat_map(|at| {
        let across = f32::from(u16::try_from(at % side).unwrap_or(0)) / 16.0 - 0.5;
        let up = f32::from(u16::try_from(at / side).unwrap_or(0)) / 16.0 - 0.5;
        [across, up, speed(), speed()]
    });

    floats(field.collect::<Vec<f32>>())
}

impl Handles<On<Start>> for Simulator {
    type Sends = (Asks<Path<Renderer>, ShapeOf, Nothing>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Self::Making { window } = *self else {
            return Ok(());
        };
        turn.to(path::<Renderer>(())).ask(&ShapeOf::pass::<Main>(Frame::Main(window)), &Nothing)?;

        Ok(())
    }
}

impl Handles<Answer<Shaped, Nothing>> for Simulator {
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

/// What the field is bound as at `access`: storage of particles, sixteen bytes each, read with `arrayLength`.
const fn field(access: Access) -> Binding {
    Binding { group: 0, binding: 0, kind: BindingKind::Storage { bytes: 16, access } }
}

impl Handles<Answer<Meshed, Learned>> for Simulator {
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
        let dots = CreateProgram {
            wgsl,
            vertex: String::from("place"),
            fragment: String::from("shade"),
            layout: VertexLayout { stride: 8, attributes: vec![corner] },
            instances: PerInstance::None,
            bindings: vec![field(Access::Read)],
            reads: Vec::new(),
            state: Fixed { blend: Blend::Opaque, depth: Depth::Off, cull: Cull::None, coverage: Coverage::Off },
            shape: learned.shape,
        };
        turn.to(path::<Renderer>(())).ask(&dots, &mesh)?;

        Ok(())
    }
}

impl Handles<Answer<Programmed, MeshId>> for Simulator {
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
        let wgsl = match turn.put(Vec::from(SIMULATE)) {
            Ok(wgsl) => wgsl,
            Err(why) => {
                say(format!("no compute program asked: {why}"));
                return Ok(());
            }
        };
        let simulate = CreateCompute { wgsl, entry: String::from("simulate"), bindings: vec![field(Access::Write)] };
        turn.to(path::<Renderer>(())).ask(&simulate, &Drawing { mesh, dots })?;

        Ok(())
    }
}

impl Handles<Answer<Computed, Drawing>> for Simulator {
    type Sends = (Asks<Path<Renderer>, CreateStorage, MadePrograms>,);

    fn handle(
        &mut self,
        (answered, drawing): (Answered<Computed>, Drawing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Computed::Made(simulate)) = answered else {
            say(format!("the compute program was not made: {answered:?}"));
            return Ok(());
        };
        let bytes = match turn.put(seeded()) {
            Ok(bytes) => bytes,
            Err(why) => {
                say(format!("no field asked: {why}"));
                return Ok(());
            }
        };
        let made = MadePrograms { mesh: drawing.mesh, dots: drawing.dots, simulate };
        turn.to(path::<Renderer>(())).ask(&CreateStorage { bytes }, &made)?;

        Ok(())
    }
}

impl Handles<Answer<Stored, MadePrograms>> for Simulator {
    type Sends = (Asks<Path<Renderer>, Place, Placing>,);

    fn handle(
        &mut self,
        (answered, made): (Answered<Stored>, MadePrograms),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Stored::Made(storage)) = answered else {
            say(format!("the field was not made: {answered:?}"));
            return Ok(());
        };
        let Self::Making { window } = *self else {
            return Ok(());
        };
        let bindings = vec![Bind { group: 0, binding: 0, to: Resource::Storage(storage) }];
        let place = Place {
            mesh: made.mesh,
            instances: Drawn::Count(PARTICLES),
            joins: vec![Join::to::<Main>(Frame::Main(window), made.dots, bindings)],
        };
        turn.to(path::<Renderer>(())).ask(&place, &Placing { simulate: made.simulate, storage })?;

        Ok(())
    }
}

impl Handles<Answer<Placed, Placing>> for Simulator {
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
        let storage = placing.storage;
        let bindings = vec![Bind { group: 0, binding: 0, to: Resource::Storage(storage) }];
        // Run in the compute pass before the one the dots are drawn in, so each frame draws where it moved them.
        let dispatch = CreateDispatch {
            join: Dispatch::to::<Compute>(Frame::Main(window), placing.simulate, bindings),
            size: Invocations::PerElement(storage),
        };
        turn.to(path::<Renderer>(())).ask(&dispatch, &Dispatching { storage })?;

        Ok(())
    }
}

impl Handles<Answer<Dispatched, Dispatching>> for Simulator {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, dispatching): (Answered<Dispatched>, Dispatching),
        _: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Dispatched::Made(_)) = answered else {
            say(format!("the field was not dispatched: {answered:?}"));
            return Ok(());
        };
        say(format!("placed dispatched particles={PARTICLES}"));
        *self = Self::Running { storage: dispatching.storage, drawn: 0 };

        Ok(())
    }
}

impl Handles<On<Tick>> for Simulator {
    type Sends = (Asks<Path<Renderer>, ReadStorage, Nothing>, Asks<Path<Renderer>, TimeFrame, Nothing>);

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        match *self {
            Self::Running { storage, drawn } if drawn < SETTLED => {
                *self = Self::Running { storage, drawn: drawn + 1 };
            }
            Self::Running { storage, .. } => {
                turn.to(path::<Renderer>(())).ask(&ReadStorage { storage }, &Nothing)?;
                *self = Self::Reading { storage };
            }
            Self::Read { storage, first, drawn } if drawn < SETTLED => {
                *self = Self::Read { storage, first, drawn: drawn + 1 };
            }
            Self::Read { storage, first, .. } => {
                turn.to(path::<Renderer>(())).ask(&ReadStorage { storage }, &Nothing)?.ask(&TimeFrame, &Nothing)?;
                *self = Self::Asked { first };
            }
            Self::Making { .. } | Self::Reading { .. } | Self::Asked { .. } => {}
        }

        Ok(())
    }
}

impl Handles<Answer<Bytes, Nothing>> for Simulator {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, _): (Answered<Bytes>, Nothing),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Bytes::Read { bytes }) = answered else {
            say(format!("the field was not read: {answered:?}"));
            return Ok(());
        };
        let read = Digest::of(turn.read(&bytes));
        match *self {
            Self::Reading { storage } => {
                let seeded = Digest::of(&seeded());
                let moved = read != seeded;
                let (from, to) = (seeded.x(), read.x());
                say(format!("first read moved_from_seed={moved} x_seeded={from} x_read={to}"));
                *self = Self::Read { storage, first: read, drawn: 0 };
            }
            Self::Asked { first } => {
                let moved = read != first;
                let (from, to) = (first.x(), read.x());
                say(format!("second read moved={moved} x_first={from} x_second={to}"));
            }
            Self::Making { .. } | Self::Running { .. } | Self::Read { .. } => {}
        }

        Ok(())
    }
}

impl Handles<Answer<Timed, Nothing>> for Simulator {
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
