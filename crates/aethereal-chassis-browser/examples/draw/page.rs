//! The page's entry points, and an actor that places a ring of triangles and moves them at each `Tick`.

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, Level, On, Path, Render, Start, Tells,
    Tick, Top, Turn, path,
};
use aethereal_chassis_browser::{Browser, Performance, open, renderer};
use aethereal_data::{Bound, Kind, KindName, Plain};
use aethereal_render::Renderer;
use aethereal_render::instances::{CreateInstances, Instanced, InstancesId, WriteInstances};
use aethereal_render::mesh::{Attribute, CreateMesh, MeshId, Meshed, VertexFormat, VertexLayout};
use aethereal_render::placement::{Bind, Drawn, Place, Placed, Resource};
use aethereal_render::program::{
    Binding, BindingKind, Blend, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId, Programmed,
};
use aethereal_render::uniforms::{CreateUniforms, Uniformed, UniformsId, WriteUniforms};
use aethereal_substrate::Engine;
use core::cell::RefCell;
use core::f32::consts::TAU;
use core::mem;
use core::num::NonZeroUsize;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::wasm_bindgen;
use web_sys::HtmlCanvasElement;

thread_local! {
    /// What the drawer was answered, for the page to show.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Notes what the drawer was answered.
fn say(line: String) {
    SAID.with_borrow_mut(|said| said.push(line));
}

/// The program the page runs, for its `requestAnimationFrame` to call.
#[wasm_bindgen]
pub struct Program {
    browser: Browser<Performance>,
}

/// Opens WebGPU on `canvas`, then makes the engine with the renderer, handed the GPU, and the drawer as its roots.
///
/// # Errors
///
/// When WebGPU will not open on the canvas, or a root does not begin.
#[wasm_bindgen]
pub async fn start(canvas: HtmlCanvasElement) -> Result<Program, JsError> {
    let gpu = open(canvas).await?;
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 1 << 20, 1 << 20, 0)
        .with_cycle::<(Tick, Render)>()
        .map_err(|unordered| JsError::new(&format!("the cycle is out of order: {unordered:?}")))?;
    let (rendering, mappings) = renderer(&mut setup, gpu);
    let engine = setup
        .add_root::<Renderer>(&(), rendering)
        .map_err(|refused| JsError::new(&format!("the renderer did not begin: {refused:?}")))?
        .add_root::<Drawer>(&(), Nothing)
        .map_err(|refused| JsError::new(&format!("the drawer did not begin: {refused:?}")))?
        .ready();

    Ok(Program { browser: Browser::new(engine, Performance, mappings) })
}

#[wasm_bindgen]
impl Program {
    /// Runs one frame.
    pub fn frame(&mut self) {
        self.browser.frame();
    }

    /// What the drawer was answered since this was last asked, a line each.
    #[must_use]
    #[expect(clippy::unused_self, reason = "the page asks it of the program it started")]
    pub fn said(&self) -> Vec<String> {
        SAID.with_borrow_mut(mem::take)
    }
}

/// An empty config.
#[derive(Plain)]
struct Nothing;

impl Kind for Nothing {
    const NAME: KindName = KindName::new("aether.example.nothing");
}

/// The most triangles in the ring.
const RING: u8 = 64;

/// A program that places each corner of a triangle, drawn at the uniform scale, at its instance's place, and shades
/// it its instance's colour.
const SHADE: &str = "
@group(0) @binding(0) var<uniform> look: vec4<f32>;

struct Between {
    @builtin(position) at: vec4<f32>,
    @location(0) colour: vec4<f32>,
}

@vertex
fn place(@location(0) corner: vec2<f32>, @location(1) offset: vec2<f32>, @location(2) colour: vec4<f32>) -> Between {
    return Between(vec4(corner * look.x + offset, 0.0, 1.0), colour);
}

@fragment
fn shade(between: Between) -> @location(0) vec4<f32> {
    return between.colour;
}
";

/// The drawer: it makes what it draws one ask after another, each answer carrying what was made so far, places it,
/// and from then writes the ring at each `Tick`.
#[derive(Bound)]
enum Drawer {
    /// Its asks are under way, and nothing is placed.
    Making,
    /// The ring is placed: these are the instances and uniforms it writes.
    Drawing { instances: InstancesId, uniforms: UniformsId },
}

impl Kind for Drawer {
    const NAME: KindName = KindName::new("aether.example.drawer");
}

impl ChildOf for Drawer {
    type Parent = Top;
}

/// What was made before the program was asked for.
#[derive(Plain)]
struct MadeMesh {
    mesh: MeshId,
}

impl Kind for MadeMesh {
    const NAME: KindName = KindName::new("aether.example.made_mesh");
}

/// What was made before the instances were asked for.
#[derive(Plain)]
struct MadeProgram {
    mesh: MeshId,
    program: ProgramId,
}

impl Kind for MadeProgram {
    const NAME: KindName = KindName::new("aether.example.made_program");
}

/// What was made before the uniforms were asked for.
#[derive(Plain)]
struct MadeInstances {
    mesh: MeshId,
    program: ProgramId,
    instances: InstancesId,
}

impl Kind for MadeInstances {
    const NAME: KindName = KindName::new("aether.example.made_instances");
}

/// What the placement draws with, kept until it is placed.
#[derive(Plain)]
struct MadeAll {
    instances: InstancesId,
    uniforms: UniformsId,
}

impl Kind for MadeAll {
    const NAME: KindName = KindName::new("aether.example.made_all");
}

impl Actor for Drawer {
    type Config = Nothing;
    type Key = ();
    type Handles = (
        On<Start>,
        On<Tick>,
        Answer<Meshed, Nothing>,
        Answer<Programmed, MadeMesh>,
        Answer<Instanced, MadeProgram>,
        Answer<Uniformed, MadeInstances>,
        Answer<Placed, MadeAll>,
    );
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Nothing, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();
        turn.subscribe::<Tick>();

        Ok(Self::Making)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// The bytes of `values`, as a blob is put.
fn bytes(values: impl IntoIterator<Item = u32>) -> Vec<u8> {
    values.into_iter().flat_map(u32::to_ne_bytes).collect()
}

/// The ring `millis` into the program: how many triangles it has then, from 32 to 63, one more each tenth of a
/// second, and their records, each a place of two floats and a colour of four bytes, turning about the middle once in
/// twenty seconds.
fn ring(millis: u64) -> (u32, Vec<u8>) {
    let count = 32 + u8::try_from(millis / 100 % 32).unwrap_or(0);
    let turned = f32::from(u16::try_from(millis % 20_000).unwrap_or(0)) / 20_000.0 * TAU;
    let records = (0..count)
        .flat_map(|at| {
            let angle = (f32::from(at) / f32::from(RING)).mul_add(TAU, turned);
            let colour = u32::from_ne_bytes([255, at * 4, 255 - at * 4, 255]);
            bytes([(angle.cos() * 0.6).to_bits(), (angle.sin() * 0.6).to_bits(), colour])
        })
        .collect();

    (u32::from(count), records)
}

/// The uniforms `millis` into the program: the scale the triangles are drawn at, and three floats the program does
/// not read.
fn look(millis: u64) -> Vec<u8> {
    let seconds = f32::from(u16::try_from(millis % 6283).unwrap_or(0)) / 1000.0;
    let scale = 0.3_f32.mul_add(seconds.sin(), 1.0);

    bytes([scale.to_bits(), 0, 0, 0])
}

impl Handles<On<Start>> for Drawer {
    type Sends = (Asks<Path<Renderer>, CreateMesh, Nothing>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let corners = [0.0_f32, 0.05, -0.04, -0.03, 0.04, -0.03].map(f32::to_bits);
        let (vertices, indices) = match (turn.put(bytes(corners)), turn.put(bytes([0, 1, 2]))) {
            (Ok(vertices), Ok(indices)) => (vertices, indices),
            (Err(why), _) | (_, Err(why)) => {
                say(format!("nothing asked: {why}"));
                return Ok(());
            }
        };
        let position = Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 };
        let triangle = CreateMesh { layout: VertexLayout { stride: 8, attributes: vec![position] }, vertices, indices };
        turn.to(path::<Renderer>(())).ask(&triangle, &Nothing)?;

        Ok(())
    }
}

/// How a record of the ring is laid out: a place, then a colour.
fn record() -> VertexLayout {
    let offset = Attribute { location: 1, format: VertexFormat::Float32x2, offset: 0 };
    let colour = Attribute { location: 2, format: VertexFormat::Unorm8x4, offset: 8 };

    VertexLayout { stride: 12, attributes: vec![offset, colour] }
}

impl Handles<Answer<Meshed, Nothing>> for Drawer {
    type Sends = (Asks<Path<Renderer>, CreateProgram, MadeMesh>,);

    fn handle(
        &mut self,
        (answered, _): (Answered<Meshed>, Nothing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Meshed::Made(mesh)) = answered else {
            say(format!("the triangle was not made: {answered:?}"));
            return Ok(());
        };
        let position = Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 };
        let shade = CreateProgram {
            wgsl: String::from(SHADE),
            vertex: String::from("place"),
            fragment: String::from("shade"),
            layout: VertexLayout { stride: 8, attributes: vec![position] },
            instances: PerInstance::Records(record()),
            bindings: vec![Binding { group: 0, binding: 0, kind: BindingKind::Uniform { bytes: 16 } }],
            state: Fixed { blend: Blend::Opaque, depth: Depth::Off, cull: Cull::None },
        };
        turn.to(path::<Renderer>(())).ask(&shade, &MadeMesh { mesh })?;

        Ok(())
    }
}

impl Handles<Answer<Programmed, MadeMesh>> for Drawer {
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
        let records = match turn.put(ring(0).1) {
            Ok(records) => records,
            Err(why) => {
                say(format!("no instances asked: {why}"));
                return Ok(());
            }
        };
        let instances = CreateInstances { layout: record(), capacity: u32::from(RING), records };
        turn.to(path::<Renderer>(())).ask(&instances, &MadeProgram { mesh: made.mesh, program })?;

        Ok(())
    }
}

impl Handles<Answer<Instanced, MadeProgram>> for Drawer {
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
        let bytes = match turn.put(look(0)) {
            Ok(bytes) => bytes,
            Err(why) => {
                say(format!("no uniforms asked: {why}"));
                return Ok(());
            }
        };
        let made = MadeInstances { mesh: made.mesh, program: made.program, instances };
        turn.to(path::<Renderer>(())).ask(&CreateUniforms { bytes }, &made)?;

        Ok(())
    }
}

impl Handles<Answer<Uniformed, MadeInstances>> for Drawer {
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
        let place = Place {
            program: made.program,
            mesh: made.mesh,
            instances: Drawn::Records(made.instances),
            bindings: vec![Bind { group: 0, binding: 0, to: Resource::Uniforms(uniforms) }],
        };
        turn.to(path::<Renderer>(())).ask(&place, &MadeAll { instances: made.instances, uniforms })?;

        Ok(())
    }
}

impl Handles<Answer<Placed, MadeAll>> for Drawer {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, made): (Answered<Placed>, MadeAll),
        _: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        match answered {
            Answered::Yes(Placed::Made(placement)) => {
                say(format!("placed {placement:?}"));
                *self = Self::Drawing { instances: made.instances, uniforms: made.uniforms };
            }
            Answered::Yes(Placed::Refused(why)) => say(format!("the ring was not placed: {why:?}")),
            Answered::No(why) => say(format!("the ring was not placed: {why:?}")),
        }

        Ok(())
    }
}

impl Handles<On<Tick>> for Drawer {
    type Sends = (Tells<Path<Renderer>, WriteInstances>, Tells<Path<Renderer>, WriteUniforms>);

    fn handle(&mut self, tick: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Self::Drawing { instances, uniforms } = *self else {
            return Ok(());
        };
        let millis = tick.elapsed_nanos / 1_000_000;
        // Written whole, then cut to the ring's length: a write that ends short of what they held leaves the rest.
        let (length, records) = ring(millis);
        let put = (turn.put(records), turn.put(Vec::new()), turn.put(look(millis)));
        let (Ok(records), Ok(nothing), Ok(bytes)) = put else {
            turn.log(Level::Warn, "a frame was not written: its bytes were not put");
            return Ok(());
        };
        turn.to(path::<Renderer>(()))
            .send(&WriteInstances { instances, first: 0, records })?
            .send(&WriteInstances { instances, first: length, records: nothing })?
            .send(&WriteUniforms { uniforms, first: 0, bytes })?;

        Ok(())
    }
}
