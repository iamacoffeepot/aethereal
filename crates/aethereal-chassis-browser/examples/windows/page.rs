//! The page's entry points, and a scene for each of two canvases, each its own window, drawing its own main frame,
//! beneath a stage whose draw list calls each, the program's root list (ADR-0055).

use crate::said::{Program, say};
use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, Level, On, Path, Render, Tells, Tick,
    Top, Turn, child, path,
};
use aethereal_chassis_browser::{Browser, WindowsSettings};
use aethereal_data::{Bound, Plain, TypedKey};
use aethereal_render::Renderer;
use aethereal_render::draw_list::{Area, DrawList, Entry, Pointer};
use aethereal_render::frame::{Frame, ShapeOf, Shaped};
use aethereal_render::instances::{CreateInstances, Instanced, InstancesId, WriteInstances};
use aethereal_render::mesh::{Attribute, CreateMesh, MeshId, Meshed, VertexFormat, VertexLayout};
use aethereal_render::pass::Main;
use aethereal_render::placement::{Bind, Drawn, Join, Place, Placed, PlacementId, Resource};
use aethereal_render::program::{
    Binding, BindingKind, Blend, Coverage, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId, Programmed,
};
use aethereal_render::shape::Shape;
use aethereal_render::uniforms::{CreateUniforms, Uniformed, UniformsId, WriteUniforms};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_window::{
    self as window, Event, InitialWindows, KeyEvent, KeySet, Keyed, Logical, OpenWindow, Opened, Surface, Window,
    WindowId, Windows,
};
use core::f32::consts::TAU;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::wasm_bindgen;

/// Bootstraps the program on the page with the window root, which opens the canvases `first` and `second` as windows
/// `0` and `1`, the renderer, which opens WebGPU on them, and the stage, the root list, with a scene for each window.
///
/// # Errors
///
/// When a canvas or WebGPU on them will not open, the page will not be listened to, or a root does not begin.
#[wasm_bindgen(wasm_bindgen_futures = aethereal_chassis_browser::wasm_bindgen_futures)]
pub async fn start() -> Result<Program, JsError> {
    let started = Browser::program()
        .with_cycle::<(Tick, Render)>()
        .root_with::<Windows>(WindowsSettings {
            canvases: vec![String::from("first"), String::from("second")],
            ..Default::default()
        })
        .root::<Renderer>()
        .root::<Stage>()
        .draws::<(Stage,)>()
        .run()?
        .await?;

    Ok(Program::from(started))
}

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

/// An empty context.
#[derive(Plain)]
#[kind(name = "aether.example.nothing")]
struct Nothing;

/// A scene's config: the window it draws on, how many triangles its ring has, and whether they are shaded warm, from
/// red, or cool, from blue.
#[derive(Plain)]
#[kind(name = "aether.example.shows")]
struct Shows {
    window: WindowId,
    count: u8,
    warm: bool,
}

/// The stage's config: what each scene shows.
#[derive(Plain)]
#[kind(name = "aether.example.staged")]
struct Staged {
    scenes: Vec<Shows>,
}

/// A scene for each window the window root opened: the first's ring full and warm, each other's a quarter as full and
/// cool.
impl<H: Host> Bootstrap<H> for Stage {
    const SECTION: &'static str = "stage";
    type Settings = ();
    type Needs = (InitialWindows,);
    type Provides = ();

    async fn bootstrap(
        (): (),
        (windows,): (InitialWindows,),
        cx: &mut Bootstrapping<'_, H, Self>,
    ) -> Result<(), Unbuilt> {
        let scenes = windows
            .all()
            .map(|window| {
                let first = window == windows.first();
                let count = if first {
                    RING
                } else {
                    RING / 4
                };
                Shows { window, count, warm: first }
            })
            .collect();

        cx.begin(&(), Staged { scenes })
    }
}

/// The stage: it makes a scene for each window, and its draw list calls each, so what each declares is in the
/// sequence (ADR-0055).
#[derive(Bound)]
#[kind(name = "aether.example.stage")]
struct Stage;

impl ChildOf for Stage {
    type Parent = Top;
}

impl Actor for Stage {
    type Config = Staged;
    type Key = ();
    type Handles = ();
    type BeginSends = (Tells<Path<Renderer>, DrawList>,);
    type EndSends = ();

    fn begin(staged: Staged, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let mut entries = Vec::with_capacity(staged.scenes.len());
        for shows in staged.scenes {
            let window = shows.window;
            if let Err(refused) = turn.spawn::<Scene>(&window, shows) {
                turn.log(Level::Error, format_args!("the scene of window {window:?} was refused: {refused}"));
            }
            entries.push(Entry::call(child::<Scene>(window), Area::WHOLE));
        }
        turn.to(path::<Renderer>(())).send(&DrawList { entries })?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A scene, keyed by the window it draws in: it asks the shape of its window's main pass, makes what it draws one ask
/// after another, each answer carrying what was made so far, places it and lists it, and from then writes the ring at
/// each `Tick`,
/// about the pointer. It subscribes to its window, declares every key of it in its draw list, and says each resize it
/// hears.
#[derive(Bound)]
#[kind(name = "aether.example.scene")]
struct Scene {
    window: WindowId,
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
}

/// What the scene has made of what it draws.
#[derive(Bound)]
enum Ring {
    /// Its asks are under way, and nothing is placed.
    Making,
    /// The ring is placed, and listed: these are the placement, and the instances and uniforms it writes.
    Drawing { placement: PlacementId, instances: InstancesId, uniforms: UniformsId },
}

impl ChildOf for Scene {
    type Parent = Stage;
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
        Keyed,
        Answer<Shaped, Nothing>,
        Answer<Meshed, Learned>,
        Answer<Programmed, MadeMesh>,
        Answer<Instanced, MadeProgram>,
        Answer<Uniformed, MadeInstances>,
        Answer<Placed, MadeAll>,
        Answer<Opened, Nothing>,
    );
    type BeginSends = (
        Tells<Path<Window>, window::Subscribe>,
        Tells<Path<Renderer>, DrawList>,
        Asks<Path<Renderer>, ShapeOf, Nothing>,
    );
    type EndSends = ();

    fn begin(shows: Shows, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Tick>();
        let main = Frame::Main(shows.window);
        turn.to(path::<Window>(shows.window)).send(&window::Subscribe)?;
        turn.to(path::<Renderer>(())).ask(&ShapeOf::pass::<Main>(main), &Nothing)?;
        let scene = Self {
            window: shows.window,
            count: shows.count,
            warm: shows.warm,
            ring: Ring::Making,
            width: 1,
            height: 1,
            middle_x: 0.0,
            middle_y: 0.0,
            held: 0,
            turned_millis: 0,
        };
        turn.to(path::<Renderer>(())).send(&scene.list())?;

        Ok(scene)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Scene {
    /// Its draw list: the ring, once it is placed, then every key of its window, heard and let through.
    fn list(&self) -> DrawList {
        let hears = Entry::Keys { frame: Frame::Main(self.window), keys: KeySet::All, blocks: false };
        let entries = match self.ring {
            Ring::Making => vec![hears],
            Ring::Drawing { placement, .. } => vec![Entry::Draw { placement, pointer: Pointer::Ignores }, hears],
        };

        DrawList { entries }
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
}

/// The keys its list hears: a key held turns the ring the other way, and `N` asks the window root for another window,
/// which a page answers `NoHost`.
impl Handles<Keyed> for Scene {
    type Sends = (Asks<Path<Windows>, OpenWindow, Nothing>,);

    fn handle(&mut self, keyed: Keyed, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        match keyed.event {
            KeyEvent::KeyDown { code, repeat, .. } => {
                if !repeat {
                    self.held += 1;
                }
                let another = code == "KeyN" && !repeat;
                if another {
                    let open =
                        OpenWindow { title: String::from("another"), size: Logical { width: 480.0, height: 360.0 } };
                    turn.to(path::<Windows>(())).ask(&open, &Nothing)?;
                }
            }
            KeyEvent::KeyUp { .. } => self.held = self.held.saturating_sub(1),
            KeyEvent::Text { .. } => {}
        }

        Ok(())
    }
}

/// The window's events: the pointer moves the ring's middle, and each resize is said beside the window, so a canvas
/// resized is seen to be its window alone.
impl Handles<Event> for Scene {
    type Sends = ();

    fn handle(&mut self, event: Event, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        match event {
            Event::PointerMoved { x, y } => {
                // A surface is some thousands of pixels across, which an f32 holds exactly.
                #[allow(clippy::cast_precision_loss)]
                let (width, height) = (self.width.max(1) as f32, self.height.max(1) as f32);
                self.middle_x = (x / width).mul_add(2.0, -1.0);
                self.middle_y = (y / height).mul_add(-2.0, 1.0);
            }
            Event::Resized(Surface { width, height, scale }) => {
                say(format!("resized window={} width={width} height={height} scale={scale}", self.window.to_name()));
                (self.width, self.height) = (width, height);
            }
            Event::FocusLost => self.held = 0,
            Event::PointerDown { .. }
            | Event::PointerUp { .. }
            | Event::Wheel { .. }
            | Event::KeyDown { .. }
            | Event::KeyUp { .. }
            | Event::Text { .. }
            | Event::Moded(_)
            | Event::FocusGained
            | Event::CloseRequested
            | Event::Closed => {}
        }

        Ok(())
    }
}

/// What the window root answered the window asked for, said beside the window whose scene asked: a page opens no
/// window on request, so it is refused, `NoHost`.
impl Handles<Answer<Opened, Nothing>> for Scene {
    type Sends = ();

    fn handle(&mut self, (answered, _): (Answered<Opened>, Nothing), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let asked_by = self.window.to_name();
        match answered {
            Answered::Yes(Opened::Window { window, .. }) => {
                say(format!("opened window={} asked_by={asked_by}", window.to_name()));
            }
            Answered::Yes(Opened::Refused(why)) => say(format!("refused open asked_by={asked_by} why={why:?}")),
            Answered::No(why) => say(format!("unanswered open asked_by={asked_by} why={why:?}")),
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
            say(format!("the window's main pass has no shape: {answered:?}"));
            return Ok(());
        };
        let corners = [0.0_f32, 0.05, -0.04, -0.03, 0.04, -0.03].map(f32::to_bits);
        let (Ok(vertices), Ok(indices)) = (turn.put(bytes(corners)), turn.put(bytes([0, 1, 2]))) else {
            say(String::from("the triangle was not asked for: its bytes were not put"));
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
            say(format!("the triangle was not made: {answered:?}"));
            return Ok(());
        };
        let Ok(wgsl) = turn.put(Vec::from(SHADE)) else {
            say(String::from("the program was not asked for: its text was not put"));
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
            say(format!("the program was not made: {answered:?}"));
            return Ok(());
        };
        let Ok(records) = turn.put(ring(0, self.count, self.warm)) else {
            say(String::from("the instances were not asked for: their records were not put"));
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
            say(format!("the instances were not made: {answered:?}"));
            return Ok(());
        };
        let Ok(bytes) = turn.put(self.look()) else {
            say(String::from("the uniforms were not asked for: their bytes were not put"));
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
            say(format!("the uniforms were not made: {answered:?}"));
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
    type Sends = (Tells<Path<Renderer>, DrawList>,);

    fn handle(
        &mut self,
        (answered, made): (Answered<Placed>, MadeAll),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        match answered {
            Answered::Yes(Placed::Made(placement)) => {
                self.ring = Ring::Drawing { placement, instances: made.instances, uniforms: made.uniforms };
                turn.to(path::<Renderer>(())).send(&self.list())?;
            }
            Answered::Yes(Placed::Refused(why)) => {
                say(format!("the ring was not placed: {why:?}"));
            }
            Answered::No(why) => say(format!("the ring was not placed: {why:?}")),
        }

        Ok(())
    }
}

impl Handles<On<Tick>> for Scene {
    type Sends = (Tells<Path<Renderer>, WriteInstances>, Tells<Path<Renderer>, WriteUniforms>);

    fn handle(&mut self, tick: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let step_millis = tick.delta_nanos / 1_000_000;
        self.turned_millis = if self.held > 0 {
            self.turned_millis.wrapping_add(TURN_MILLIS).saturating_sub(step_millis) % TURN_MILLIS
        } else {
            self.turned_millis.wrapping_add(step_millis) % TURN_MILLIS
        };
        let Ring::Drawing { instances, uniforms, .. } = self.ring else {
            return Ok(());
        };
        let records = ring(self.turned_millis, self.count, self.warm);
        let (Ok(records), Ok(bytes)) = (turn.put(records), turn.put(self.look())) else {
            say(String::from("a frame was not written: its bytes were not put"));
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
