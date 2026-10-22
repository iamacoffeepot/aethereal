//! Windows opened while the program runs, each drawing its own main frame: a ring of triangles of its own count and
//! colours, turning, following the pointer over that window, and turning the other way while a key is held in that
//! window, with the frames each was handed timed. The program opens one window as it is set up; `N` in any window asks
//! the window root for another, and the director that asked makes a scene for each window it is answered with, keyed
//! by the window, which hears only its own window (ADR-0027).
//!
//! Run with `cargo run --release -p aethereal-chassis-desktop --example windows -- --seconds 3`. With `--seconds N`
//! the director asks for a second window as the program starts, and the program quits itself after N seconds; each
//! scene prints the time between the frames its window was handed, from one `Tick` to the next, as `key=value` in
//! milliseconds: their mean, median, 99th percentile, and longest. Each answer to a window asked for is printed as it
//! comes. A window's close button closes that window alone, and its scene prints its frames as it closes; the program
//! ends once the last window has closed, or Escape is pressed in any. Each key, button, and turn of the wheel a window
//! hears is printed as it comes, beside the window, and each resize.

#![allow(clippy::print_stdout, clippy::print_stderr)] // An example prints what it heard and how its frames were paced.

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, Level, On, Path, Render, Start, Tells,
    Tick, Top, Turn, path,
};
use aethereal_chassis_desktop::Desktop;
use aethereal_chassis_headless::Exit;
use aethereal_data::config::Overlay;
use aethereal_data::{Bound, Plain, TypedKey};
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
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_window::{
    self as window, Event, InitialWindows, Logical, OpenWindow, Opened, Surface, Window, WindowId, Windows,
};
use core::error::Error;
use core::f32::consts::TAU;
use core::num::NonZeroUsize;
use std::env;
use std::process::ExitCode;
use std::thread;

/// The most triangles in a ring.
const RING: u8 = 48;

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
        Ok(exit) => {
            println!("exit={exit:?}");
            ExitCode::SUCCESS
        }
        Err(failed) => {
            eprintln!("{failed}");
            ExitCode::FAILURE
        }
    }
}

/// Runs the program of the window root, which opens one window, the renderer, which draws on it, and the director, on
/// as many workers as the machine has cores, at most four, the engine's views in a part for each, each scene quitting
/// after `--seconds` when they are any.
fn run() -> Result<Exit, Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let seconds = args
        .iter()
        .position(|arg| arg == "--seconds")
        .and_then(|at| args.get(at + 1))
        .and_then(|seconds| seconds.parse::<u64>().ok())
        .unwrap_or(0);
    let cores = thread::available_parallelism().map_or(1, NonZeroUsize::get);
    let workers = cores.min(4);

    let program = Desktop::program()
        .with_cycle::<(Tick, Render)>()
        .map_err(|unordered| format!("the cycle is out of order: {unordered:?}"))?
        .root::<Windows>()
        .root::<Renderer>()
        .root::<Director>();
    let parts = program.parts()?;
    let mut code = Overlay::new("code");
    let sets = [
        format!("engine.workers={workers}"),
        format!("engine.parts={workers}"),
        String::from(r#"windows.windows=[{"title":"aethereal: first","width":640,"height":480}]"#),
        format!("director.seconds={seconds}"),
    ];
    for set in sets {
        code.set(&parts, &set)?;
    }

    Ok(program.layer(code).run()?)
}

/// How long each scene runs: the director's section, `director`.
#[derive(Plain, Default)]
#[kind(name = "aether.example.show")]
#[settings]
struct Show {
    /// The seconds each scene runs for, none for as long as it is let.
    seconds: u64,
}

/// The director's config: the window opened as the program was set up, and how many seconds each scene runs for,
/// none for as long as it is let.
#[derive(Plain)]
#[kind(name = "aether.example.directs")]
struct Directs {
    first: WindowId,
    seconds: u64,
}

/// The director makes its first scene for the first window the window root opened.
impl<H: Host> Bootstrap<H> for Director {
    const SECTION: &'static str = "director";
    type Settings = Show;
    type Needs = (InitialWindows,);
    type Provides = ();

    async fn bootstrap(
        show: Show,
        (windows,): (InitialWindows,),
        cx: &mut Bootstrapping<'_, H, Self>,
    ) -> Result<(), Unbuilt> {
        cx.begin(&(), Directs { first: windows.first(), seconds: show.seconds })
    }
}

/// The director: it makes a scene for the window opened as the program was set up, asks the window root for another
/// window on `N` in any window, and for one as the program starts when it runs for some seconds, and makes a scene for
/// each window it is answered with, each of its own count and colours.
#[derive(Bound)]
#[kind(name = "aether.example.director")]
struct Director {
    seconds: u64,
    /// How many scenes it has made.
    scenes: u8,
}

impl ChildOf for Director {
    type Parent = Top;
}

impl Director {
    /// Makes a scene for `window`, and subscribes to the window, to hear `N` in it.
    fn direct(
        &mut self,
        window: WindowId,
        turn: &mut Turn<'_, Self, (Tells<Path<Window>, window::Subscribe>,)>,
    ) -> Result<(), Fault> {
        let shows = Shows {
            window,
            seconds: self.seconds,
            count: RING >> (self.scenes % 3),
            warm: self.scenes.is_multiple_of(2),
        };
        self.scenes += 1;
        if let Err(refused) = turn.spawn::<Scene>(&window, shows) {
            turn.log(Level::Error, format_args!("the scene of window {window:?} was refused: {refused}"));
        }
        turn.to(path::<Window>(window)).send(&window::Subscribe)?;

        Ok(())
    }
}

/// The window asked for: titled after the window that asked, at a size in logical pixels.
fn another() -> OpenWindow {
    OpenWindow { title: String::from("aethereal: another"), size: Logical { width: 480.0, height: 480.0 } }
}

impl Actor for Director {
    type Config = Directs;
    type Key = ();
    type Handles = (On<Start>, Event, Answer<Opened, Nothing>);
    type BeginSends = (Tells<Path<Window>, window::Subscribe>,);
    type EndSends = ();

    fn begin(directs: Directs, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();
        let shows = Shows { window: directs.first, seconds: directs.seconds, count: RING, warm: true };
        if let Err(refused) = turn.spawn::<Scene>(&directs.first, shows) {
            turn.log(Level::Error, format_args!("the first scene was refused: {refused}"));
        }
        turn.to(path::<Window>(directs.first)).send(&window::Subscribe)?;

        Ok(Self { seconds: directs.seconds, scenes: 1 })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// Asks for a second window as the program starts, when it runs for some seconds, so a timed run shows a window
/// opened on request.
impl Handles<On<Start>> for Director {
    type Sends = (Asks<Path<Windows>, OpenWindow, Nothing>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if self.seconds > 0 {
            turn.to(path::<Windows>(())).ask(&another(), &Nothing)?;
        }

        Ok(())
    }
}

/// `N` in any window asks for another; Escape in any quits.
impl Handles<Event> for Director {
    type Sends = (Asks<Path<Windows>, OpenWindow, Nothing>,);

    fn handle(&mut self, event: Event, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Event::KeyDown { code, repeat: false, .. } = event else {
            return Ok(());
        };
        match code.as_str() {
            "KeyN" => {
                turn.to(path::<Windows>(())).ask(&another(), &Nothing)?;
            }
            "Escape" => turn.quit(),
            _ => {}
        }

        Ok(())
    }
}

/// The window asked for, printed as it is answered, and a scene made for it once it is open.
impl Handles<Answer<Opened, Nothing>> for Director {
    type Sends = (Tells<Path<Window>, window::Subscribe>,);

    fn handle(
        &mut self,
        (answered, _): (Answered<Opened>, Nothing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        match answered {
            Answered::Yes(Opened::Window { window, surface }) => {
                let Surface { width, height, scale } = surface;
                println!("opened window={} width={width} height={height} scale={scale}", window.to_name());
                self.direct(window, turn)?;
            }
            Answered::Yes(Opened::Refused(why)) => println!("refused open why={why:?}"),
            Answered::No(why) => println!("unanswered open why={why:?}"),
        }

        Ok(())
    }
}

/// An empty context.
#[derive(Plain)]
#[kind(name = "aether.example.nothing")]
struct Nothing;

/// A scene's config: the window it draws on, how many seconds it runs for, none for as long as it is let, how many
/// triangles its ring has, and whether they are shaded warm, from red, or cool, from blue.
#[derive(Plain)]
#[kind(name = "aether.example.shows")]
struct Shows {
    window: WindowId,
    seconds: u64,
    count: u8,
    warm: bool,
}

/// A scene, keyed by the window it draws in: it asks the shape of its window's main pass as it begins, makes what it
/// draws one ask after another, each answer carrying what was made so far, places it, and from then writes the ring at
/// each `Tick`, about the pointer. It subscribes to its window, prints what it hears, and keeps the time between each
/// `Tick` and the last, which it prints as its window closes, and ends.
#[derive(Bound)]
#[kind(name = "aether.example.scene")]
struct Scene {
    window: WindowId,
    seconds: u64,
    count: u8,
    warm: bool,
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
    type Parent = Director;
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
    type Key = WindowId;
    type Handles = (
        On<Tick>,
        Event,
        Answer<Shaped, Nothing>,
        Answer<Meshed, Learned>,
        Answer<Programmed, MadeMesh>,
        Answer<Instanced, MadeProgram>,
        Answer<Uniformed, MadeInstances>,
        Answer<Placed, MadeAll>,
    );
    type BeginSends = (Tells<Path<Window>, window::Subscribe>, Asks<Path<Renderer>, ShapeOf, Nothing>);
    type EndSends = ();

    fn begin(shows: Shows, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Tick>();
        turn.to(path::<Window>(shows.window)).send(&window::Subscribe)?;
        turn.to(path::<Renderer>(())).ask(&ShapeOf::pass::<Main>(Frame::Main(shows.window)), &Nothing)?;

        Ok(Self {
            window: shows.window,
            seconds: shows.seconds,
            count: shows.count,
            warm: shows.warm,
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

/// A ring of `count` triangles turned `turned_millis`, shaded warm or cool: each triangle's record, a place of two
/// floats and a colour of four bytes, about the middle, once round in [`TURN_MILLIS`].
fn ring(turned_millis: u64, count: u8, warm: bool) -> Vec<u8> {
    let turned = f32::from(u16::try_from(turned_millis % TURN_MILLIS).unwrap_or(0)) / 4_000.0 * TAU;
    (0..count)
        .flat_map(|at| {
            let angle = (f32::from(at) / f32::from(count)).mul_add(TAU, turned);
            let shade = at * (240 / count.max(1));
            let channels = if warm {
                [255, shade, 255 - shade, 255]
            } else {
                [255 - shade, shade, 255, 255]
            };
            let colour = u32::from_ne_bytes(channels);
            bytes([(angle.cos() * 0.4).to_bits(), (angle.sin() * 0.4).to_bits(), colour])
        })
        .collect()
}

impl Scene {
    /// The uniforms: the scale the triangles are drawn at, the ring's middle, and a float the program does not read.
    fn look(&self) -> Vec<u8> {
        bytes([1.0_f32.to_bits(), self.middle_x.to_bits(), self.middle_y.to_bits(), 0])
    }

    /// Prints the time between the frames its window was handed, in milliseconds, beside the window.
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
            "window={} frames={count} mean_millis={:.3} p50_millis={:.3} p99_millis={:.3} max_millis={:.3}",
            self.window.to_name(),
            millis(mean),
            millis(median),
            millis(tail),
            millis(longest)
        );
    }
}

impl Handles<Event> for Scene {
    type Sends = ();

    fn handle(&mut self, event: Event, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let window = self.window.to_name();
        match event {
            Event::PointerMoved { x, y } => {
                // A surface is some thousands of pixels across, which an f32 holds exactly.
                #[allow(clippy::cast_precision_loss)]
                let (width, height) = (self.width.max(1) as f32, self.height.max(1) as f32);
                self.middle_x = (x / width).mul_add(2.0, -1.0);
                self.middle_y = (y / height).mul_add(-2.0, 1.0);
            }
            Event::PointerDown { x, y, button } => {
                println!("pointer_down window={window} x={x} y={y} button={button:?}");
            }
            Event::PointerUp { x, y, button } => println!("pointer_up window={window} x={x} y={y} button={button:?}"),
            Event::Wheel { x, y, delta_x, delta_y } => {
                println!("wheel window={window} x={x} y={y} delta_x={delta_x} delta_y={delta_y}");
            }
            Event::KeyDown { code, key, repeat } => {
                println!("key_down window={window} code={code} key={key:?} repeat={repeat}");
                if !repeat {
                    self.held += 1;
                }
            }
            Event::KeyUp { code, key } => {
                println!("key_up window={window} code={code} key={key:?}");
                self.held = self.held.saturating_sub(1);
            }
            Event::Resized(surface) => {
                let Surface { width, height, scale } = surface;
                println!("resized window={window} width={width} height={height} scale={scale}");
                (self.width, self.height) = (surface.width, surface.height);
            }
            Event::FocusGained => println!("focus=gained window={window}"),
            Event::FocusLost => {
                println!("focus=lost window={window}");
                self.held = 0;
            }
            Event::CloseRequested => println!("close_requested window={window}"),
            Event::Closed => {
                println!("closed window={window}");
                self.print_intervals();
                turn.close();
            }
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
        let Ok(records) = turn.put(ring(0, self.count, self.warm)) else {
            turn.log(Level::Error, "the instances were not asked for: their records were not put");
            return Ok(());
        };
        let instances = CreateInstances { layout: record(), capacity: u32::from(self.count), records };
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
        let records = ring(self.turned_millis, self.count, self.warm);
        let (Ok(records), Ok(bytes)) = (turn.put(records), turn.put(self.look())) else {
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
