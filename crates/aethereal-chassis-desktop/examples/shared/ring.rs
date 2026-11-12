//! The scene of the window examples: a ring of triangles on the first window, turning, following the pointer, and
//! turning the other way while a key is held, which prints what its window hears and times the frames it was handed.
//! With `--views two` the window shows two views of the one ring side by side, the right one drawn with a look of its
//! own, its triangles smaller: each view a frame that follows half the window, and the window's main frame laying them
//! out (ADR-0049). Its turn and its triangles' sizes are tuned while it runs (ADR-0050).

use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, Level, On, Path, ProtocolRef,
    ReadTuning, Start, Tells, Tick, ToSender, Top, Tunable, Tune, Tuned, Tunes, Tuning, Turn, path, tuning,
};
use aethereal_data::{Bound, Plain};
use aethereal_render::Renderer;
use aethereal_render::draw_list::{DrawList, Entry};
use aethereal_render::effect::{CreateEffect, Effected};
use aethereal_render::frame::{
    CreateFrame, DepthUse, Does, Ends, Frame, FrameId, Framed, Load, PassSpec, Scale, ShapeOf, Shaped, Target,
};
use aethereal_render::instances::{CreateInstances, Instanced, InstancesId, WriteInstances};
use aethereal_render::mesh::{Attribute, CreateMesh, MeshId, Meshed, VertexFormat, VertexLayout};
use aethereal_render::pass::{Main, Pass};
use aethereal_render::placement::{Bind, Drawn, Filter, Join, Place, Placed, Resource, Sampling, Wrap};
use aethereal_render::program::{
    Binding, BindingKind, Blend, Coverage, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId, Programmed,
};
use aethereal_render::shape::Shape;
use aethereal_render::texture::{TextureFormat, TextureId};
use aethereal_render::uniforms::{CreateUniforms, Uniformed, UniformsId, WriteUniforms};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_window::{self as window, Event, InitialWindows, KeyEvent, KeySet, Keyed, Window, WindowId};
use core::f32::consts::TAU;

/// The triangles in the ring.
const RING: u8 = 48;

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

/// An effect that lays two views side by side over the window, the left over its left half and the right over its
/// right, with a thin line between them.
const LAY: &str = "
@group(1) @binding(0) var left: texture_2d<f32>;
@group(1) @binding(1) var right: texture_2d<f32>;
@group(1) @binding(2) var smoothly: sampler;

@fragment
fn lay(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let at = vec2(fract(uv.x * 2.0), uv.y);
    let near = textureSample(left, smoothly, at);
    let far = textureSample(right, smoothly, at);
    let line = abs(uv.x - 0.5) < 0.001;
    return select(select(far, near, uv.x < 0.5), vec4(1.0), line);
}
";

/// The pass the window's main frame lays the two views out in.
#[derive(Plain)]
#[kind(name = "aether.example.lay")]
pub struct Lay;

impl Pass for Lay {}

/// What the ring is tuned by while it turns (ADR-0050).
#[derive(Plain, Debug, Clone, Copy, PartialEq)]
#[kind(name = "aether.example.ring_tuning")]
#[settings]
pub struct RingTuning {
    /// How long one turn of the ring takes, in milliseconds: from 100 to 600000.
    turn_millis: u64,
    /// The scale the triangles are drawn at in the window's one view, or the left of two: above 0, at most 4.
    near: f32,
    /// The scale the triangles are drawn at in the right of two views, with `--views two`: above 0, at most 4.
    far: f32,
}

impl Default for RingTuning {
    fn default() -> Self {
        Self { turn_millis: 4_000, near: 1.0, far: 0.6 }
    }
}

/// How long the scene runs: its section, `scene`.
#[derive(Plain, Default)]
#[kind(name = "aether.example.show")]
#[settings]
pub struct Show {
    /// How long the scene runs: `forever`, until its window is closed or Escape is pressed, or `for:3`, three
    /// seconds, then the program quits.
    #[flag]
    runs: Runs,
    /// How many views of the ring the window shows: `one`, or `two`, side by side, the right one's triangles smaller.
    #[flag]
    views: Views,
}

/// How many views of the ring the window shows.
#[derive(Plain, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Views {
    /// One, the window's own frame.
    #[default]
    One,
    /// Two side by side, each a frame that follows half the window.
    Two,
}

/// How long the scene runs.
#[derive(Plain, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Runs {
    /// For as long as it is let: until its window is closed or Escape is pressed.
    #[default]
    Forever,
    /// For this many seconds, then the program quits itself.
    For { seconds: u64 },
}

impl Runs {
    /// Whether a run `elapsed_nanos` long is over.
    const fn over(self, elapsed_nanos: u64) -> bool {
        match self {
            Self::Forever => false,
            Self::For { seconds } => elapsed_nanos >= seconds.saturating_mul(1_000_000_000),
        }
    }
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
        cx.begin(&(), Shows { window: windows.first(), runs: show.runs, views: show.views })
    }
}

/// An empty context.
#[derive(Plain)]
#[kind(name = "aether.example.nothing")]
pub struct Nothing;

/// The scene's config: the window it draws on, how long it runs, and how many views of the ring it shows.
#[derive(Plain)]
#[kind(name = "aether.example.shows")]
pub struct Shows {
    window: WindowId,
    runs: Runs,
    views: Views,
}

/// The scene: it asks the shape of its window's main pass, makes what it draws one ask after another, each answer
/// carrying what was made so far, places it, and from then writes the ring at each `Tick`, about the pointer. It
/// subscribes to its window, declares every key of it in its draw list, as the program's root list names it
/// (ADR-0055), prints what it hears, and keeps the time between each `Tick` and the last.
#[derive(Bound)]
#[kind(name = "aether.example.scene")]
pub struct Scene {
    window: WindowId,
    runs: Runs,
    views: Views,
    onto: Onto,
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
    /// What the ring is tuned by.
    tuning: RingTuning,
    /// The nanoseconds between each `Tick` and the last.
    intervals: Vec<u64>,
}

/// What the scene has made of what it draws.
#[derive(Bound)]
enum Ring {
    /// Its asks are under way, and nothing is placed.
    Making,
    /// The ring is placed: these are the instances and uniforms it writes.
    Drawing { instances: InstancesId, looks: Looks },
}

/// What the ring is drawn onto.
#[derive(Bound, Clone, Copy)]
enum Onto {
    /// The window's main frame, the renderer's own.
    Window,
    /// Two views, each a frame that follows half the window, which the window's main frame lays out.
    Views { left: FrameId, right: FrameId },
}

/// The uniforms the ring is drawn with: one look, or one for each view.
#[derive(Plain, Clone, Copy)]
pub enum Looks {
    One(UniformsId),
    Two { near: UniformsId, far: UniformsId },
}

impl ChildOf for Scene {
    type Parent = Top;
}

/// What was learned before the mesh was asked for: the shape the program draws into.
#[derive(Plain)]
#[kind(name = "aether.example.learned")]
pub struct Learned {
    shape: Shape,
}

/// What was made before the program was asked for.
#[derive(Plain)]
#[kind(name = "aether.example.made_mesh")]
pub struct MadeMesh {
    shape: Shape,
    mesh: MeshId,
}

/// What was made before the instances were asked for.
#[derive(Plain)]
#[kind(name = "aether.example.made_program")]
pub struct MadeProgram {
    mesh: MeshId,
    program: ProgramId,
}

/// What was made before each look was asked for: the first, then the far view's, beside the first.
#[derive(Plain)]
#[kind(name = "aether.example.looking")]
pub enum Looking {
    First { mesh: MeshId, program: ProgramId, instances: InstancesId },
    Far { mesh: MeshId, program: ProgramId, instances: InstancesId, near: UniformsId },
}

/// What the placement draws with, kept until it is placed.
#[derive(Plain)]
#[kind(name = "aether.example.made_all")]
pub struct MadeAll {
    instances: InstancesId,
    looks: Looks,
}

/// Which pass's shape was asked for: the window's main pass, or the views' main pass, which both views share.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
#[kind(name = "aether.example.shaping")]
pub enum Shaping {
    Window,
    Views,
}

/// Which frame was asked for, with what was made before it: the left view, after the effect that lays the views out;
/// the right view, after the left; and the window's main frame, which lays both out.
#[derive(Plain, Debug)]
#[kind(name = "aether.example.framing")]
pub enum Framing {
    Left { lay: ProgramId },
    Right { lay: ProgramId, left: FrameId, texture: TextureId },
    Main { left: FrameId, right: FrameId },
}

impl Actor for Scene {
    type Config = Shows;
    type Key = ();
    type Handles = (
        On<Start>,
        On<Tick>,
        Event,
        Keyed,
        ReadTuning,
        Tune,
        Answer<Shaped, Shaping>,
        Answer<Effected, Nothing>,
        Answer<Framed, Framing>,
        Answer<Meshed, Learned>,
        Answer<Programmed, MadeMesh>,
        Answer<Instanced, MadeProgram>,
        Answer<Uniformed, Looking>,
        Answer<Placed, MadeAll>,
    );
    type BeginSends = (Tells<Path<Window>, window::Subscribe>, Tells<Path<Renderer>, DrawList>);
    type EndSends = ();

    fn begin(shows: Shows, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();
        turn.subscribe::<Tick>();
        turn.to(path::<Window>(shows.window)).send(&window::Subscribe)?;
        let hears = Entry::Keys { frame: Frame::Main(shows.window), keys: KeySet::All, blocks: false };
        turn.to(path::<Renderer>(())).send(&DrawList { entries: vec![hears] })?;

        Ok(Self {
            window: shows.window,
            runs: shows.runs,
            views: shows.views,
            onto: Onto::Window,
            ring: Ring::Making,
            width: 1,
            height: 1,
            middle_x: 0.0,
            middle_y: 0.0,
            held: 0,
            turned_millis: 0,
            tuning: RingTuning::default(),
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

/// The ring turned `turned_millis` of a turn `turn_millis` long: each triangle's record, a place of two floats and a
/// colour of four bytes, about the middle.
fn ring(turned_millis: u64, turn_millis: u64) -> Vec<u8> {
    // A turn is at most ten minutes of milliseconds, which an f64 holds exactly, and a fraction of a turn an f32 holds
    // closely enough to draw.
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    let turned = ((turned_millis % turn_millis) as f64 / turn_millis as f64) as f32 * TAU;
    (0..RING)
        .flat_map(|at| {
            let angle = (f32::from(at) / f32::from(RING)).mul_add(TAU, turned);
            let colour = u32::from_ne_bytes([255, at * 5, 255 - at * 5, 255]);
            bytes([(angle.cos() * 0.4).to_bits(), (angle.sin() * 0.4).to_bits(), colour])
        })
        .collect()
}

impl Scene {
    /// The uniforms: the scale the triangles are drawn at, `scale`, the ring's middle, and a float the program does
    /// not read.
    fn look(&self, scale: f32) -> Vec<u8> {
        bytes([scale.to_bits(), self.middle_x.to_bits(), self.middle_y.to_bits(), 0])
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
    type Sends = (Asks<Path<Renderer>, ShapeOf, Shaping>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.to(path::<Renderer>(())).ask(&ShapeOf::pass::<Main>(Frame::Main(self.window)), &Shaping::Window)?;

        Ok(())
    }
}

/// Each key and text its list hears is printed; a key held turns the ring the other way, and Escape quits.
impl Handles<Keyed> for Scene {
    type Sends = ();

    fn handle(&mut self, keyed: Keyed, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        match keyed.event {
            KeyEvent::KeyDown { code, key, repeat } => {
                println!("key_down code={code} key={key:?} repeat={repeat}");
                if !repeat {
                    self.held += 1;
                }
                if code == "Escape" {
                    turn.quit();
                }
            }
            KeyEvent::KeyUp { code, key } => {
                println!("key_up code={code} key={key:?}");
                self.held = self.held.saturating_sub(1);
            }
            KeyEvent::Text { text } => println!("text text={text:?}"),
        }

        Ok(())
    }
}

impl Handles<Event> for Scene {
    type Sends = ();

    fn handle(&mut self, event: Event, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        match event {
            Event::PointerMoved { x, y } => {
                // A surface is some thousands of pixels across, which an f32 holds exactly.
                #[allow(clippy::cast_precision_loss)]
                let (width, height) = (self.width.max(1) as f32, self.height.max(1) as f32);
                // Two views each follow the pointer across their own half.
                let (x, width) = match self.views {
                    Views::One => (x, width),
                    Views::Two => (x % (width / 2.0), width / 2.0),
                };
                self.middle_x = (x / width).mul_add(2.0, -1.0);
                self.middle_y = (y / height).mul_add(-2.0, 1.0);
            }
            Event::PointerDown { x, y, button } => println!("pointer_down x={x} y={y} button={button:?}"),
            Event::PointerUp { x, y, button } => println!("pointer_up x={x} y={y} button={button:?}"),
            Event::Wheel { x, y, delta_x, delta_y } => {
                println!("wheel x={x} y={y} delta_x={delta_x} delta_y={delta_y}");
            }
            Event::Resized(surface) => {
                println!("resized width={} height={} scale={}", surface.width, surface.height, surface.scale);
                (self.width, self.height) = (surface.width, surface.height);
            }
            Event::Moded(mode) => println!("moded mode={mode:?}"),
            Event::FocusGained => println!("focus=gained"),
            Event::FocusLost => {
                println!("focus=lost");
                self.held = 0;
            }
            Event::CloseRequested => println!("close=requested"),
            Event::Closed => println!("closed"),
            // Keys and text come through the scene's list, never raw (ADR-0055).
            Event::KeyDown { .. } | Event::KeyUp { .. } | Event::Text { .. } => {}
        }

        Ok(())
    }
}

impl Handles<Answer<Shaped, Shaping>> for Scene {
    type Sends = (Asks<Path<Renderer>, CreateMesh, Learned>, Asks<Path<Renderer>, CreateEffect, Nothing>);

    fn handle(
        &mut self,
        (answered, shaping): (Answered<Shaped>, Shaping),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Shaped::Shape(shape)) = answered else {
            turn.log(Level::Error, format_args!("the pass the ring joins has no shape: {answered:?}"));
            return Ok(());
        };
        // The window's shape is what the views are laid out in; the views' is what the ring is drawn in.
        let lays = shaping == Shaping::Window && self.views == Views::Two;
        if lays {
            let Ok(wgsl) = turn.put(Vec::from(LAY)) else {
                turn.log(Level::Error, "the effect was not asked for: its text was not put");
                return Ok(());
            };
            let bindings = vec![
                Binding { group: 1, binding: 0, kind: BindingKind::Texture },
                Binding { group: 1, binding: 1, kind: BindingKind::Texture },
                Binding { group: 1, binding: 2, kind: BindingKind::Sampler },
            ];
            let lay = CreateEffect {
                wgsl,
                fragment: String::from("lay"),
                reads: Vec::new(),
                bindings,
                blend: Blend::Opaque,
                shape: Shape::flat(shape.colour),
            };
            turn.to(path::<Renderer>(())).ask(&lay, &Nothing)?;
            return Ok(());
        }
        let Some(triangle) = triangle(turn) else {
            return Ok(());
        };
        turn.to(path::<Renderer>(())).ask(&triangle, &Learned { shape })?;

        Ok(())
    }
}

/// A view of the ring: a frame that follows half the window, drawing the ring onto its texture in its one pass,
/// `Main`, from black.
fn view(window: WindowId) -> CreateFrame {
    let ring = Does::Draw { into: Target::End, depth: DepthUse::Off, load: Load::Clear, reads: Vec::new() };

    CreateFrame {
        ends: Ends::Follows { window, width: Scale::Half, height: Scale::Full, format: TextureFormat::Rgba8 },
        targets: Vec::new(),
        passes: vec![PassSpec::of::<Main>(ring)],
    }
}

impl Handles<Answer<Effected, Nothing>> for Scene {
    type Sends = (Asks<Path<Renderer>, CreateFrame, Framing>,);

    fn handle(
        &mut self,
        (answered, _): (Answered<Effected>, Nothing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Effected::Made(lay)) = answered else {
            turn.log(Level::Error, format_args!("the effect that lays the views out was not made: {answered:?}"));
            return Ok(());
        };
        turn.to(path::<Renderer>(())).ask(&view(self.window), &Framing::Left { lay: lay.program() })?;

        Ok(())
    }
}

impl Handles<Answer<Framed, Framing>> for Scene {
    type Sends = (Asks<Path<Renderer>, CreateFrame, Framing>, Asks<Path<Renderer>, ShapeOf, Shaping>);

    fn handle(
        &mut self,
        (answered, framing): (Answered<Framed>, Framing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        match (answered, framing) {
            (Answered::Yes(Framed::Texture { frame, texture }), Framing::Left { lay }) => {
                let right = Framing::Right { lay, left: frame, texture };
                turn.to(path::<Renderer>(())).ask(&view(self.window), &right)?;
            }
            (Answered::Yes(Framed::Texture { frame, texture }), Framing::Right { lay, left, texture: shown }) => {
                let smoothly = Sampling { filter: Filter::Linear, wrap: Wrap::Clamp };
                let lay = Does::Effect {
                    program: lay,
                    reads: Vec::new(),
                    into: Target::End,
                    bindings: vec![
                        Bind { group: 1, binding: 0, to: Resource::Texture(shown) },
                        Bind { group: 1, binding: 1, to: Resource::Texture(texture) },
                        Bind { group: 1, binding: 2, to: Resource::Sampler(smoothly) },
                    ],
                };
                let main = CreateFrame {
                    ends: Ends::Window(self.window),
                    targets: Vec::new(),
                    passes: vec![PassSpec::of::<Lay>(lay)],
                };
                turn.to(path::<Renderer>(())).ask(&main, &Framing::Main { left, right: frame })?;
            }
            (Answered::Yes(Framed::Made(_)), Framing::Main { left, right }) => {
                self.onto = Onto::Views { left, right };
                turn.to(path::<Renderer>(())).ask(&ShapeOf::pass::<Main>(Frame::Texture(left)), &Shaping::Views)?;
            }
            (answered, framing) => {
                turn.log(Level::Error, format_args!("a frame was not made: {framing:?}: {answered:?}"));
            }
        }

        Ok(())
    }
}

/// The triangle each of the ring's instances draws, its bytes put: none, logged, when they were not.
fn triangle<S>(turn: &mut Turn<'_, Scene, S>) -> Option<CreateMesh> {
    let corners = [0.0_f32, 0.05, -0.04, -0.03, 0.04, -0.03].map(f32::to_bits);
    let (Ok(vertices), Ok(indices)) = (turn.put(bytes(corners)), turn.put(bytes([0, 1, 2]))) else {
        turn.log(Level::Error, "the triangle was not asked for: its bytes were not put");
        return None;
    };
    let position = Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 };

    Some(CreateMesh { layout: VertexLayout { stride: 8, attributes: vec![position] }, vertices, indices })
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
        let Ok(records) = turn.put(ring(0, self.tuning.turn_millis)) else {
            turn.log(Level::Error, "the instances were not asked for: their records were not put");
            return Ok(());
        };
        let instances = CreateInstances { layout: record(), capacity: u32::from(RING), records };
        turn.to(path::<Renderer>(())).ask(&instances, &MadeProgram { mesh: made.mesh, program })?;

        Ok(())
    }
}

impl Handles<Answer<Instanced, MadeProgram>> for Scene {
    type Sends = (Asks<Path<Renderer>, CreateUniforms, Looking>,);

    fn handle(
        &mut self,
        (answered, made): (Answered<Instanced>, MadeProgram),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Instanced::Made(instances)) = answered else {
            turn.log(Level::Error, format_args!("the instances were not made: {answered:?}"));
            return Ok(());
        };
        let Ok(bytes) = turn.put(self.look(self.tuning.near)) else {
            turn.log(Level::Error, "the uniforms were not asked for: their bytes were not put");
            return Ok(());
        };
        let first = Looking::First { mesh: made.mesh, program: made.program, instances };
        turn.to(path::<Renderer>(())).ask(&CreateUniforms { bytes }, &first)?;

        Ok(())
    }
}

impl Handles<Answer<Uniformed, Looking>> for Scene {
    type Sends = (Asks<Path<Renderer>, Place, MadeAll>, Asks<Path<Renderer>, CreateUniforms, Looking>);

    fn handle(
        &mut self,
        (answered, looking): (Answered<Uniformed>, Looking),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Uniformed::Made(uniforms)) = answered else {
            turn.log(Level::Error, format_args!("a look was not made: {answered:?}"));
            return Ok(());
        };
        let (mesh, instances, joins, looks) = match (looking, self.onto) {
            (Looking::First { mesh, program, instances }, Onto::Window) => {
                let joins = vec![join(Frame::Main(self.window), program, uniforms)];
                (mesh, instances, joins, Looks::One(uniforms))
            }
            (Looking::First { mesh, program, instances }, Onto::Views { .. }) => {
                let Ok(bytes) = turn.put(self.look(self.tuning.far)) else {
                    turn.log(Level::Error, "the far view's look was not asked for: its bytes were not put");
                    return Ok(());
                };
                let far = Looking::Far { mesh, program, instances, near: uniforms };
                turn.to(path::<Renderer>(())).ask(&CreateUniforms { bytes }, &far)?;
                return Ok(());
            }
            (Looking::Far { mesh, program, instances, near }, Onto::Views { left, right }) => {
                let joins =
                    vec![join(Frame::Texture(left), program, near), join(Frame::Texture(right), program, uniforms)];
                (mesh, instances, joins, Looks::Two { near, far: uniforms })
            }
            (Looking::Far { .. }, Onto::Window) => {
                turn.log(Level::Error, "the far view's look was made with no views to draw in");
                return Ok(());
            }
        };
        let place = Place { mesh, instances: Drawn::Records(instances), joins };
        turn.to(path::<Renderer>(())).ask(&place, &MadeAll { instances, looks })?;

        Ok(())
    }
}

/// The ring's join of the pass `Main` of `frame`, drawn by `program` with the look `uniforms`.
fn join(frame: Frame, program: ProgramId, uniforms: UniformsId) -> Join {
    let bindings = vec![Bind { group: 0, binding: 0, to: Resource::Uniforms(uniforms) }];

    Join::to::<Main>(frame, program, bindings)
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
                self.ring = Ring::Drawing { instances: made.instances, looks: made.looks };
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
        let ran = self.runs.over(tick.elapsed_nanos);
        if ran {
            self.print_intervals();
            turn.quit();
        }

        let step_millis = tick.delta_nanos / 1_000_000;
        let turn_millis = self.tuning.turn_millis;
        self.turned_millis = if self.held > 0 {
            self.turned_millis.wrapping_add(turn_millis).saturating_sub(step_millis) % turn_millis
        } else {
            self.turned_millis.wrapping_add(step_millis) % turn_millis
        };
        let Ring::Drawing { instances, looks } = self.ring else {
            return Ok(());
        };
        let Ok(records) = turn.put(ring(self.turned_millis, turn_millis)) else {
            turn.log(Level::Warn, "a frame was not written: its bytes were not put");
            return Ok(());
        };
        turn.to(path::<Renderer>(())).send(&WriteInstances { instances, first: 0, records })?;
        let written = match looks {
            Looks::One(uniforms) => vec![(uniforms, self.tuning.near)],
            Looks::Two { near, far } => vec![(near, self.tuning.near), (far, self.tuning.far)],
        };
        for (uniforms, scale) in written {
            let Ok(bytes) = turn.put(self.look(scale)) else {
                turn.log(Level::Warn, "a look was not written: its bytes were not put");
                return Ok(());
            };
            turn.to(path::<Renderer>(())).send(&WriteUniforms { uniforms, first: 0, bytes })?;
        }

        Ok(())
    }
}

/// The ring is tuned by its turn and its triangles' sizes, each held to what it can draw.
impl Tunable for Scene {
    type Tuning = RingTuning;

    fn tuning(&self) -> RingTuning {
        self.tuning
    }

    fn tune(&mut self, tuning: RingTuning) -> Result<(), String> {
        let turns = (100..=600_000).contains(&tuning.turn_millis);
        let sized = |scale: f32| scale > 0.0 && scale <= 4.0;
        let fits = sized(tuning.near) && sized(tuning.far);
        match (turns, fits) {
            (false, _) => Err(String::from("`turn_millis` is from 100 to 600000")),
            (_, false) => Err(String::from("`near` and `far` are above 0 and at most 4")),
            (true, true) => {
                self.tuning = tuning;
                Ok(())
            }
        }
    }

    fn tunes(me: ActorRef<Self>) -> ProtocolRef<Tunes> {
        me.protocol()
    }
}

impl Handles<ReadTuning> for Scene {
    type Sends = (ToSender<Tuning>,);

    fn handle(&mut self, _: ReadTuning, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        tuning::read(self, turn)
    }
}

impl Handles<Tune> for Scene {
    type Sends = (ToSender<Tuned>,);

    fn handle(&mut self, tune: Tune, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        tuning::tune(self, &tune, turn)
    }
}
