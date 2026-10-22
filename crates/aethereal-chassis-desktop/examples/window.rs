//! A ring of triangles in a window, turning, following the pointer, and turning the other way while a key is held,
//! with the frames it was handed timed.
//!
//! Run with `cargo run --release -p aethereal-chassis-desktop --example window -- --seconds 3`. With `--seconds N`
//! the program quits itself after N seconds, and prints the time between the frames it was handed, from one `Tick` to
//! the next, as `key=value` in milliseconds: their mean, median, 99th percentile, and longest. Without it, it runs until
//! its window is closed or Escape is pressed. Each key, button, and turn of the wheel it hears is printed as it comes,
//! and each resize. `--help` lists every flag the program reads, each also read from its `WINDOW_` variable.

#![allow(clippy::print_stdout, clippy::print_stderr)] // An example prints what it heard and how its frames were paced.

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, Level, On, Path, Render, Start, Tells,
    Tick, Top, Turn, path,
};
use aethereal_chassis_desktop::Desktop;
use aethereal_chassis_headless::Exit;
use aethereal_chassis_headless::native::{Launched, NativeLayers};
use aethereal_data::{Bound, Plain};
use aethereal_render::Renderer;
use aethereal_render::frame::{Frame, ShapeOf, Shaped};
use aethereal_render::instances::{CreateInstances, Instanced, InstancesId, WriteInstances};
use aethereal_render::mesh::{Attribute, CreateMesh, MeshId, Meshed, VertexFormat, VertexLayout};
use aethereal_render::pass::Main;
use aethereal_render::placement::{Bind, Drawn, Join, Place, Placed, Resource};
use aethereal_render::program::{
    Binding, BindingKind, Blend, Coverage, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId, Programmed,
};
use aethereal_render::shape::Shape;
use aethereal_render::uniforms::{CreateUniforms, Uniformed, UniformsId, WriteUniforms};
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Unbuilt};
use aethereal_window::{self as window, Event, InitialWindows, Window, WindowId, Windows};
use core::error::Error;
use core::f32::consts::TAU;
use core::num::{NonZeroU32, NonZeroUsize};
use std::process::ExitCode;
use std::thread;

/// The triangles in the ring.
const RING: u8 = 48;

/// The most workers the program runs on.
const MOST_WORKERS: NonZeroUsize = NonZeroUsize::new(4).expect("4 is not zero");

/// How long one turn of the ring takes, in milliseconds.
const TURN_MILLIS: u64 = 4_000;

/// A program that places each corner of a triangle, drawn at the uniform scale about the uniform middle, at its
/// instance's place, and shades it its instance's colour.
const SHADE: &str = "
@group(0) @binding(0) var<uniform> look: vec4<f32>;

struct Between {
    @builtin(position) at: vec4<f32>,
    @location(0) colour: vec4<f32>,
}

@vertex
fn place(@location(0) corner: vec2<f32>, @location(1) offset: vec2<f32>, @location(2) colour: vec4<f32>) -> Between {
    return Between(vec4(corner * look.x + offset + look.yz, 0.0, 1.0), colour);
}

@fragment
fn shade(between: Between) -> @location(0) vec4<f32> {
    return between.colour;
}
";

fn main() -> ExitCode {
    match run() {
        Ok(Launched::Ran(exit)) => {
            println!("exit={exit:?}");
            ExitCode::SUCCESS
        }
        Ok(Launched::Printed) => ExitCode::SUCCESS,
        Err(failed) => {
            eprintln!("{failed}");
            ExitCode::FAILURE
        }
    }
}

/// Launches the program of the window root, the renderer, which draws on the window it opened, and the scene, on as
/// many workers as the machine has cores, at most four, the engine's views in a part for each, with what its command
/// line and environment set over that.
fn run() -> Result<Launched<Exit>, Box<dyn Error>> {
    let workers = NonZeroU32::try_from(
        thread::available_parallelism().map_or(NonZeroUsize::MIN, |cores| cores.min(MOST_WORKERS)),
    )?;

    Ok(Desktop::program()
        .with_cycle::<(Tick, Render)>()
        .with_engine(EngineSettings { workers, parts: workers, ..Default::default() })
        .root::<Windows>()
        .root::<Renderer>()
        .root::<Scene>()
        .launch("window")?)
}

/// How long the scene runs: its section, `scene`.
#[derive(Plain, Default)]
#[kind(name = "aether.example.show")]
#[settings]
struct Show {
    /// The seconds the scene runs for, none for as long as it is let.
    #[flag]
    seconds: u64,
}

/// The scene draws on the first window the window root opened, for as long as its settings say.
impl<H: Host> Bootstrap<H> for Scene {
    const SECTION: &'static str = "scene";
    type Settings = Show;
    type Needs = (InitialWindows,);
    type Provides = ();

    async fn bootstrap(
        show: Show,
        (windows,): (InitialWindows,),
        cx: &mut Bootstrapping<'_, H, Self>,
    ) -> Result<(), Unbuilt> {
        cx.begin(&(), Shows { window: windows.first(), seconds: show.seconds })
    }
}

/// An empty context.
#[derive(Plain)]
#[kind(name = "aether.example.nothing")]
struct Nothing;

/// The scene's config: the window it draws on, and how many seconds it runs for, none for as long as it is let.
#[derive(Plain)]
#[kind(name = "aether.example.shows")]
struct Shows {
    window: WindowId,
    seconds: u64,
}

/// The scene: it asks the shape of its window's main pass, makes what it draws one ask after another, each answer
/// carrying what was made so far, places it, and from then writes the ring at each `Tick`, about the pointer. It
/// subscribes to its window, prints what it hears, and keeps the time between each `Tick` and the last.
#[derive(Bound)]
#[kind(name = "aether.example.scene")]
struct Scene {
    window: WindowId,
    seconds: u64,
    ring: Ring,
    /// The window's surface, in its pixels, as it was last heard.
    width: u32,
    height: u32,
    /// The ring's middle, in clip space.
    middle_x: f32,
    middle_y: f32,
    /// Keys held down: the ring turns the other way while any is.
    held: u32,
    /// How far the ring has turned, in milliseconds of turning.
    turned_millis: u64,
    /// The nanoseconds between each `Tick` and the last.
    intervals: Vec<u64>,
}

/// What the scene has made of what it draws.
#[derive(Bound)]
enum Ring {
    /// Its asks are under way, and nothing is placed.
    Making,
    /// The ring is placed: these are the instances and uniforms it writes.
    Drawing { instances: InstancesId, uniforms: UniformsId },
}

impl ChildOf for Scene {
    type Parent = Top;
}

/// What was learned before the mesh was asked for: the shape the program draws into.
#[derive(Plain)]
#[kind(name = "aether.example.learned")]
struct Learned {
    shape: Shape,
}

/// What was made before the program was asked for.
#[derive(Plain)]
#[kind(name = "aether.example.made_mesh")]
struct MadeMesh {
    shape: Shape,
    mesh: MeshId,
}

/// What was made before the instances were asked for.
#[derive(Plain)]
#[kind(name = "aether.example.made_program")]
struct MadeProgram {
    mesh: MeshId,
    program: ProgramId,
}

/// What was made before the uniforms were asked for.
#[derive(Plain)]
#[kind(name = "aether.example.made_instances")]
struct MadeInstances {
    mesh: MeshId,
    program: ProgramId,
    instances: InstancesId,
}

/// What the placement draws with, kept until it is placed.
#[derive(Plain)]
#[kind(name = "aether.example.made_all")]
struct MadeAll {
    instances: InstancesId,
    uniforms: UniformsId,
}

impl Actor for Scene {
    type Config = Shows;
    type Key = ();
    type Handles = (
        On<Start>,
        On<Tick>,
        Event,
        Answer<Shaped, Nothing>,
        Answer<Meshed, Learned>,
        Answer<Programmed, MadeMesh>,
        Answer<Instanced, MadeProgram>,
        Answer<Uniformed, MadeInstances>,
        Answer<Placed, MadeAll>,
    );
    type BeginSends = (Tells<Path<Window>, window::Subscribe>,);
    type EndSends = ();

    fn begin(shows: Shows, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();
        turn.subscribe::<Tick>();
        turn.to(path::<Window>(shows.window)).send(&window::Subscribe)?;

        Ok(Self {
            window: shows.window,
            seconds: shows.seconds,
            ring: Ring::Making,
            width: 1,
            height: 1,
            middle_x: 0.0,
            middle_y: 0.0,
            held: 0,
            turned_millis: 0,
            intervals: Vec::new(),
        })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// The bytes of `values`, as a blob is put.
fn bytes(values: impl IntoIterator<Item = u32>) -> Vec<u8> {
    values.into_iter().flat_map(u32::to_ne_bytes).collect()
}

/// The ring turned `turned_millis`: each triangle's record, a place of two floats and a colour of four bytes, about
/// the middle, once round in [`TURN_MILLIS`].
fn ring(turned_millis: u64) -> Vec<u8> {
    let turned = f32::from(u16::try_from(turned_millis % TURN_MILLIS).unwrap_or(0)) / 4_000.0 * TAU;
    (0..RING)
        .flat_map(|at| {
            let angle = (f32::from(at) / f32::from(RING)).mul_add(TAU, turned);
            let colour = u32::from_ne_bytes([255, at * 5, 255 - at * 5, 255]);
            bytes([(angle.cos() * 0.4).to_bits(), (angle.sin() * 0.4).to_bits(), colour])
        })
        .collect()
}

impl Scene {
    /// The uniforms: the scale the triangles are drawn at, the ring's middle, and a float the program does not read.
    fn look(&self) -> Vec<u8> {
        bytes([1.0_f32.to_bits(), self.middle_x.to_bits(), self.middle_y.to_bits(), 0])
    }

    /// Prints the time between the frames it was handed, in milliseconds.
    fn print_intervals(&self) {
        // The first `Tick`'s delta runs from `Start`, not from a frame before it.
        let mut sorted = self.intervals.get(1..).unwrap_or_default().to_vec();
        sorted.sort_unstable();
        let count = sorted.len();
        let at = |percent: usize| sorted.get((count * percent / 100).min(count.saturating_sub(1))).copied();
        let millis = |nanos: u64| {
            // Nanoseconds of a few seconds, which an f64 holds exactly.
            #[allow(clippy::cast_precision_loss)]
            let millis = nanos as f64 / 1e6;
            millis
        };
        let sum: u64 = sorted.iter().sum();
        let mean = u64::try_from(count).map_or(0, |count| sum.checked_div(count).unwrap_or(0));
        let (median, tail, longest) = (at(50).unwrap_or(0), at(99).unwrap_or(0), sorted.last().copied().unwrap_or(0));
        println!(
            "frames={count} mean_millis={:.3} p50_millis={:.3} p99_millis={:.3} max_millis={:.3}",
            millis(mean),
            millis(median),
            millis(tail),
            millis(longest)
        );
    }
}

impl Handles<On<Start>> for Scene {
    type Sends = (Asks<Path<Renderer>, ShapeOf, Nothing>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.to(path::<Renderer>(())).ask(&ShapeOf::pass::<Main>(Frame::Main(self.window)), &Nothing)?;

        Ok(())
    }
}

impl Handles<Event> for Scene {
    type Sends = ();

    fn handle(&mut self, event: Event, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        match event {
            Event::PointerMoved { x, y } => {
                // A surface is some thousands of pixels across, which an f32 holds exactly.
                #[allow(clippy::cast_precision_loss)]
                let (width, height) = (self.width.max(1) as f32, self.height.max(1) as f32);
                self.middle_x = (x / width).mul_add(2.0, -1.0);
                self.middle_y = (y / height).mul_add(-2.0, 1.0);
            }
            Event::PointerDown { x, y, button } => println!("pointer_down x={x} y={y} button={button:?}"),
            Event::PointerUp { x, y, button } => println!("pointer_up x={x} y={y} button={button:?}"),
            Event::Wheel { x, y, delta_x, delta_y } => {
                println!("wheel x={x} y={y} delta_x={delta_x} delta_y={delta_y}");
            }
            Event::KeyDown { code, key, repeat } => {
                println!("key_down code={code} key={key:?} repeat={repeat}");
                if !repeat {
                    self.held += 1;
                }
                if code == "Escape" {
                    turn.quit();
                }
            }
            Event::KeyUp { code, key } => {
                println!("key_up code={code} key={key:?}");
                self.held = self.held.saturating_sub(1);
            }
            Event::Resized(surface) => {
                println!("resized width={} height={} scale={}", surface.width, surface.height, surface.scale);
                (self.width, self.height) = (surface.width, surface.height);
            }
            Event::FocusGained => println!("focus=gained"),
            Event::FocusLost => {
                println!("focus=lost");
                self.held = 0;
            }
            Event::CloseRequested => println!("close=requested"),
            Event::Closed => println!("closed"),
        }

        Ok(())
    }
}

impl Handles<Answer<Shaped, Nothing>> for Scene {
    type Sends = (Asks<Path<Renderer>, CreateMesh, Learned>,);

    fn handle(
        &mut self,
        (answered, _): (Answered<Shaped>, Nothing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Shaped::Shape(shape)) = answered else {
            turn.log(Level::Error, format_args!("the window's main pass has no shape: {answered:?}"));
            return Ok(());
        };
        let corners = [0.0_f32, 0.05, -0.04, -0.03, 0.04, -0.03].map(f32::to_bits);
        let (Ok(vertices), Ok(indices)) = (turn.put(bytes(corners)), turn.put(bytes([0, 1, 2]))) else {
            turn.log(Level::Error, "the triangle was not asked for: its bytes were not put");
            return Ok(());
        };
        let position = Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 };
        let triangle = CreateMesh { layout: VertexLayout { stride: 8, attributes: vec![position] }, vertices, indices };
        turn.to(path::<Renderer>(())).ask(&triangle, &Learned { shape })?;

        Ok(())
    }
}

/// How a record of the ring is laid out: a place, then a colour.
fn record() -> VertexLayout {
    let offset = Attribute { location: 1, format: VertexFormat::Float32x2, offset: 0 };
    let colour = Attribute { location: 2, format: VertexFormat::Unorm8x4, offset: 8 };

    VertexLayout { stride: 12, attributes: vec![offset, colour] }
}

impl Handles<Answer<Meshed, Learned>> for Scene {
    type Sends = (Asks<Path<Renderer>, CreateProgram, MadeMesh>,);

    fn handle(
        &mut self,
        (answered, learned): (Answered<Meshed>, Learned),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Meshed::Made(mesh)) = answered else {
            turn.log(Level::Error, format_args!("the triangle was not made: {answered:?}"));
            return Ok(());
        };
        let Ok(wgsl) = turn.put(Vec::from(SHADE)) else {
            turn.log(Level::Error, "the program was not asked for: its text was not put");
            return Ok(());
        };
        let position = Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 };
        let shade = CreateProgram {
            wgsl,
            vertex: String::from("place"),
            fragment: String::from("shade"),
            layout: VertexLayout { stride: 8, attributes: vec![position] },
            instances: PerInstance::Records(record()),
            bindings: vec![Binding { group: 0, binding: 0, kind: BindingKind::Uniform { bytes: 16 } }],
            reads: Vec::new(),
            state: Fixed { blend: Blend::Opaque, depth: Depth::Off, cull: Cull::None, coverage: Coverage::Off },
            shape: learned.shape,
        };
        turn.to(path::<Renderer>(())).ask(&shade, &MadeMesh { shape: learned.shape, mesh })?;

        Ok(())
    }
}

impl Handles<Answer<Programmed, MadeMesh>> for Scene {
    type Sends = (Asks<Path<Renderer>, CreateInstances, MadeProgram>,);

    fn handle(
        &mut self,
        (answered, made): (Answered<Programmed>, MadeMesh),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Programmed::Made(program)) = answered else {
            turn.log(Level::Error, format_args!("the program was not made: {answered:?}"));
            return Ok(());
        };
        let Ok(records) = turn.put(ring(0)) else {
            turn.log(Level::Error, "the instances were not asked for: their records were not put");
            return Ok(());
        };
        let instances = CreateInstances { layout: record(), capacity: u32::from(RING), records };
        turn.to(path::<Renderer>(())).ask(&instances, &MadeProgram { mesh: made.mesh, program })?;

        Ok(())
    }
}

impl Handles<Answer<Instanced, MadeProgram>> for Scene {
    type Sends = (Asks<Path<Renderer>, CreateUniforms, MadeInstances>,);

    fn handle(
        &mut self,
        (answered, made): (Answered<Instanced>, MadeProgram),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Instanced::Made(instances)) = answered else {
            turn.log(Level::Error, format_args!("the instances were not made: {answered:?}"));
            return Ok(());
        };
        let Ok(bytes) = turn.put(self.look()) else {
            turn.log(Level::Error, "the uniforms were not asked for: their bytes were not put");
            return Ok(());
        };
        let made = MadeInstances { mesh: made.mesh, program: made.program, instances };
        turn.to(path::<Renderer>(())).ask(&CreateUniforms { bytes }, &made)?;

        Ok(())
    }
}

impl Handles<Answer<Uniformed, MadeInstances>> for Scene {
    type Sends = (Asks<Path<Renderer>, Place, MadeAll>,);

    fn handle(
        &mut self,
        (answered, made): (Answered<Uniformed>, MadeInstances),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Uniformed::Made(uniforms)) = answered else {
            turn.log(Level::Error, format_args!("the uniforms were not made: {answered:?}"));
            return Ok(());
        };
        let bindings = vec![Bind { group: 0, binding: 0, to: Resource::Uniforms(uniforms) }];
        let place = Place {
            mesh: made.mesh,
            instances: Drawn::Records(made.instances),
            joins: vec![Join::to::<Main>(Frame::Main(self.window), made.program, bindings)],
        };
        turn.to(path::<Renderer>(())).ask(&place, &MadeAll { instances: made.instances, uniforms })?;

        Ok(())
    }
}

impl Handles<Answer<Placed, MadeAll>> for Scene {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, made): (Answered<Placed>, MadeAll),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        match answered {
            Answered::Yes(Placed::Made(_)) => {
                self.ring = Ring::Drawing { instances: made.instances, uniforms: made.uniforms };
            }
            Answered::Yes(Placed::Refused(why)) => {
                turn.log(Level::Error, format_args!("the ring was not placed: {why:?}"));
            }
            Answered::No(why) => turn.log(Level::Error, format_args!("the ring was not placed: {why:?}")),
        }

        Ok(())
    }
}

impl Handles<On<Tick>> for Scene {
    type Sends = (Tells<Path<Renderer>, WriteInstances>, Tells<Path<Renderer>, WriteUniforms>);

    fn handle(&mut self, tick: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.intervals.push(tick.delta_nanos);
        let ran = self.seconds > 0 && tick.elapsed_nanos >= self.seconds.saturating_mul(1_000_000_000);
        if ran {
            self.print_intervals();
            turn.quit();
        }

        let step_millis = tick.delta_nanos / 1_000_000;
        self.turned_millis = if self.held > 0 {
            self.turned_millis.wrapping_add(TURN_MILLIS).saturating_sub(step_millis) % TURN_MILLIS
        } else {
            self.turned_millis.wrapping_add(step_millis) % TURN_MILLIS
        };
        let Ring::Drawing { instances, uniforms } = self.ring else {
            return Ok(());
        };
        let (Ok(records), Ok(bytes)) = (turn.put(ring(self.turned_millis)), turn.put(self.look())) else {
            turn.log(Level::Warn, "a frame was not written: its bytes were not put");
            return Ok(());
        };
        turn.to(path::<Renderer>(())).send(&WriteInstances { instances, first: 0, records })?.send(&WriteUniforms {
            uniforms,
            first: 0,
            bytes,
        })?;

        Ok(())
    }
}
