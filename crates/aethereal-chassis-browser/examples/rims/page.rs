//! The page's entry points, and an actor that makes a frame of a floor drawn at one sample writing its depth, rims drawn
//! at four samples with their alpha as their coverage, reading the floor's depth and a depth of their own, and an
//! effect that lays the rims on the floor onto the canvas, then places the floor and five rims in it.

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, BlobRef, ChildOf, Ending, Fault, Handles, On, Path, Render, Start, Tick,
    Top, Turn, path,
};
use aethereal_chassis_browser::{Browser, Started};
use aethereal_data::config::Overlay;
use aethereal_data::{Bound, Plain};
use aethereal_render::Renderer;
use aethereal_render::effect::{CreateEffect, EffectId, Effected, Read};
use aethereal_render::frame::builder::{Effect, Float, FrameBuilder, depth, samples, scale};
use aethereal_render::frame::{CreateFrame, Ends, Frame, Framed, ShapeOf, Shaped};
use aethereal_render::mesh::{Attribute, CreateMesh, MeshId, Meshed, VertexFormat, VertexLayout};
use aethereal_render::pass::{Main, Pass};
use aethereal_render::placement::{Drawn, Join, Place, Placed};
use aethereal_render::program::{
    Blend, Coverage, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId, Programmed,
};
use aethereal_render::shape::{ColourFormat, DepthFormat, Samples, Shape};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_window::{InitialWindows, WindowId, Windows};
use core::cell::RefCell;
use core::mem;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::wasm_bindgen;

thread_local! {
    /// What the rimmer was answered, for the page to show.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Notes what the rimmer was answered.
fn say(line: String) {
    SAID.with_borrow_mut(|said| said.push(line));
}

/// The program the page runs, for its `requestAnimationFrame` to call.
#[wasm_bindgen]
pub struct Program {
    started: Started,
}

/// Bootstraps the program on the page with the window root, which opens the page's canvas as the one window,
/// the renderer, which opens WebGPU on it, and the rimmer.
///
/// # Errors
///
/// When the canvas or WebGPU on it will not open, the page will not be listened to, or a root does not begin.
#[wasm_bindgen(wasm_bindgen_futures = aethereal_chassis_browser::wasm_bindgen_futures)]
pub async fn start() -> Result<Program, JsError> {
    let program = Browser::program()
        .with_cycle::<(Tick, Render)>()
        .map_err(|unordered| JsError::new(&format!("the cycle is out of order: {unordered:?}")))?
        .root::<Windows>()
        .root::<Renderer>()
        .root::<Rimmer>();
    // Its blobs hold a frame of a large canvas.
    let mut code = Overlay::new("code");
    code.set(&program.parts()?, "engine.blob_bytes=67108864")?;

    Ok(Program { started: program.layer(code).run()?.await? })
}

#[wasm_bindgen]
impl Program {
    /// Runs one frame, at the timestamp `requestAnimationFrame` handed its callback, in milliseconds.
    pub fn frame(&mut self, timestamp_millis: f64) {
        self.started.frame(timestamp_millis);
    }

    /// What the rimmer was answered since this was last asked, a line each.
    #[must_use]
    #[expect(clippy::unused_self, reason = "the page asks it of the program it started")]
    pub fn said(&self) -> Vec<String> {
        SAID.with_borrow_mut(mem::take)
    }
}

/// The rimmer draws on the one window the window root opened.
impl<H: Host> Bootstrap<H> for Rimmer {
    const SECTION: &'static str = "rimmer";
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

/// The rimmer's config: the window it draws on.
#[derive(Plain)]
#[kind(name = "aether.example.shows")]
struct Shows {
    window: WindowId,
}

/// The pass that draws the floor at one sample, writing the depth the rims read.
#[derive(Plain)]
#[kind(name = "aether.example.floor")]
struct Floor;

impl Pass for Floor {}

/// The pass that draws the rims at four samples, with a depth of their own, reading the floor's.
#[derive(Plain)]
#[kind(name = "aether.example.rims")]
struct Rims;

impl Pass for Rims {}

/// The pass that lays the rims on the floor onto the window.
#[derive(Plain)]
#[kind(name = "aether.example.lay")]
struct Lay;

impl Pass for Lay {}

/// A program that draws the quad over the lower part of the canvas as a floor that leans away: nearest at the
/// canvas's bottom, at a depth of 0.1, and farthest at its top edge, at 0.9, lighter as it goes.
const FLOOR: &str = "
struct Between {
    @builtin(position) at: vec4<f32>,
    @location(0) along: f32,
}

@vertex
fn place(@location(0) corner: vec2<f32>) -> Between {
    let along = corner.y * 0.5 + 0.5;
    return Between(vec4(corner.x, mix(-1.0, 0.3, along), mix(0.1, 0.9, along), 1.0), along);
}

@fragment
fn shade(between: Between) -> @location(0) vec4<f32> {
    let along = between.along;
    return vec4(0.10 + 0.12 * along, 0.12 + 0.16 * along, 0.16 + 0.20 * along, 1.0);
}
";

/// A program that draws the quad five times as rims, side by side and overlapping, each nearer than the one before:
/// a ring whose alpha falls from one to none across a pixel or so at its edges, which is its coverage. It reads the
/// floor's depth at its pixel and drops the fragment the floor is in front of, so each rim is cut where the floor
/// rises past its own depth; its own depth orders the rims among themselves.
const RIM: &str = "
@group(0) @binding(0) var floor: texture_depth_2d;

struct Between {
    @builtin(position) at: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) rim: u32,
}

@vertex
fn place(@location(0) corner: vec2<f32>, @builtin(instance_index) rim: u32) -> Between {
    let x = f32(rim) * 0.32 - 0.64;
    let z = 0.62 - 0.06 * f32(rim);
    return Between(vec4(x + corner.x * 0.3, -0.05 + corner.y * 0.5, z, 1.0), corner, rim);
}

@fragment
fn shade(between: Between) -> @location(0) vec4<f32> {
    let off = abs(length(between.local) - 0.78);
    let soft = fwidth(off);
    let alpha = 1.0 - smoothstep(0.07 - soft, 0.07 + soft, off);
    if (textureLoad(floor, vec2<i32>(between.at.xy), 0) < between.at.z) {
        discard;
    }
    let hue = f32(between.rim) / 5.0;
    let colour = vec3(0.95, 0.35 + 0.5 * hue, 0.9 - 0.7 * hue);
    return vec4(colour, alpha);
}
";

/// An effect that adds the rims, resolved from their four samples, to the floor: where a rim covers part of a pixel,
/// that part of its colour is added.
const LAY: &str = "
@group(0) @binding(0) var floor: texture_2d<f32>;
@group(0) @binding(1) var rims: texture_2d<f32>;

@fragment
fn lay(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(at.xy);
    return vec4(textureLoad(floor, pixel, 0).rgb + textureLoad(rims, pixel, 0).rgb, 1.0);
}
";

/// How many rims are drawn.
const RIMS: u32 = 5;

/// The rimmer: it makes what it draws one ask after another, each answer carrying what was made so far, makes its
/// frame, and places the floor and the rims.
#[derive(Bound)]
#[kind(name = "aether.example.rimmer")]
enum Rimmer {
    /// Its asks are under way, and nothing is placed on `window`.
    Making { window: WindowId },
    /// Both are placed.
    Drawing,
}

impl ChildOf for Rimmer {
    type Parent = Top;
}

/// What was learned before the quad was asked for: the window's colour, which the lay draws into.
#[derive(Plain)]
#[kind(name = "aether.example.learned")]
struct Learned {
    window: ColourFormat,
}

/// What was made before the lay was asked for: the quad.
#[derive(Plain)]
#[kind(name = "aether.example.laying")]
struct Laying {
    mesh: MeshId,
}

/// Which program was asked for, with what was made before it.
#[derive(Plain)]
#[kind(name = "aether.example.making")]
enum Making {
    /// The floor, once the lay was made.
    Floor { mesh: MeshId, lay: EffectId<(Float, Float)> },
    /// The rims, once the floor was.
    Rims { mesh: MeshId, lay: EffectId<(Float, Float)>, floor: ProgramId },
}

/// What the placements draw with, kept until the frame is made.
#[derive(Plain)]
#[kind(name = "aether.example.made_programs")]
struct MadePrograms {
    mesh: MeshId,
    floor: ProgramId,
    rims: ProgramId,
}

/// Which placement was asked for.
#[derive(Plain)]
#[kind(name = "aether.example.placing")]
enum Placing {
    /// The floor, with what the rims are then placed with.
    Floor { mesh: MeshId, rims: ProgramId },
    /// The rims.
    Rims,
}

impl Actor for Rimmer {
    type Config = Shows;
    type Key = ();
    type Handles = (
        On<Start>,
        Answer<Shaped, Nothing>,
        Answer<Meshed, Learned>,
        Answer<Effected, Laying>,
        Answer<Programmed, Making>,
        Answer<Framed, MadePrograms>,
        Answer<Placed, Placing>,
    );
    type BeginSends = ();
    type EndSends = ();

    fn begin(shows: Shows, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();

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

/// How the quad's corners are laid out: a place of two floats.
fn corners() -> VertexLayout {
    let corner = Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 };

    VertexLayout { stride: 8, attributes: vec![corner] }
}

/// A program of the WGSL the blob `wgsl` holds over the quad, testing and writing depth, reading `reads`, with its
/// alpha as `coverage` says, drawn into a pass of `shape`.
fn program(wgsl: BlobRef, reads: Vec<Read>, coverage: Coverage, shape: Shape) -> CreateProgram {
    CreateProgram {
        wgsl,
        vertex: String::from("place"),
        fragment: String::from("shade"),
        layout: corners(),
        instances: PerInstance::None,
        bindings: Vec::new(),
        reads,
        state: Fixed { blend: Blend::Opaque, depth: Depth::TestWrite, cull: Cull::None, coverage },
        shape,
    }
}

impl Handles<On<Start>> for Rimmer {
    type Sends = (Asks<Path<Renderer>, ShapeOf, Nothing>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Self::Making { window } = *self else {
            return Ok(());
        };
        turn.to(path::<Renderer>(())).ask(&ShapeOf::pass::<Main>(Frame::Main(window)), &Nothing)?;

        Ok(())
    }
}

impl Handles<Answer<Shaped, Nothing>> for Rimmer {
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
        let quad = bytes([-1.0_f32, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0].map(f32::to_bits));
        let (vertices, indices) = match (turn.put(quad), turn.put(bytes([0, 1, 2, 0, 2, 3]))) {
            (Ok(vertices), Ok(indices)) => (vertices, indices),
            (Err(why), _) | (_, Err(why)) => {
                say(format!("nothing asked: {why}"));
                return Ok(());
            }
        };
        let quad = CreateMesh { layout: corners(), vertices, indices };
        turn.to(path::<Renderer>(())).ask(&quad, &Learned { window: shape.colour })?;

        Ok(())
    }
}

impl Handles<Answer<Meshed, Learned>> for Rimmer {
    type Sends = (Asks<Path<Renderer>, CreateEffect, Laying>,);

    fn handle(
        &mut self,
        (answered, learned): (Answered<Meshed>, Learned),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Meshed::Made(mesh)) = answered else {
            say(format!("the quad was not made: {answered:?}"));
            return Ok(());
        };
        let wgsl = match turn.put(Vec::from(LAY)) {
            Ok(wgsl) => wgsl,
            Err(why) => {
                say(format!("no lay asked: {why}"));
                return Ok(());
            }
        };
        let lay = CreateEffect {
            wgsl,
            fragment: String::from("lay"),
            reads: vec![Read::Float, Read::Float],
            bindings: Vec::new(),
            blend: Blend::Opaque,
            shape: Shape::flat(learned.window),
        };
        turn.to(path::<Renderer>(())).ask(&lay, &Laying { mesh })?;

        Ok(())
    }
}

impl Handles<Answer<Effected, Laying>> for Rimmer {
    type Sends = (Asks<Path<Renderer>, CreateProgram, Making>,);

    fn handle(
        &mut self,
        (answered, Laying { mesh }): (Answered<Effected>, Laying),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Effected::Made(effect)) = answered else {
            say(format!("the lay was not made: {answered:?}"));
            return Ok(());
        };
        let Some(lay) = EffectId::of(&effect) else {
            say(format!("the lay reads what it was not asked to: {:?}", effect.reads()));
            return Ok(());
        };
        let wgsl = match turn.put(Vec::from(FLOOR)) {
            Ok(wgsl) => wgsl,
            Err(why) => {
                say(format!("no floor asked: {why}"));
                return Ok(());
            }
        };
        let floor = program(wgsl, Vec::new(), Coverage::Off, Shape::with_depth(ColourFormat::Rgba16Float));
        turn.to(path::<Renderer>(())).ask(&floor, &Making::Floor { mesh, lay })?;

        Ok(())
    }
}

impl Handles<Answer<Programmed, Making>> for Rimmer {
    type Sends = (Asks<Path<Renderer>, CreateProgram, Making>, Asks<Path<Renderer>, CreateFrame, MadePrograms>);

    fn handle(
        &mut self,
        (answered, making): (Answered<Programmed>, Making),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Programmed::Made(made)) = answered else {
            say(format!("a program was not made: {answered:?}"));
            return Ok(());
        };
        let (mesh, lay, floor) = match making {
            Making::Floor { mesh, lay } => {
                let wgsl = match turn.put(Vec::from(RIM)) {
                    Ok(wgsl) => wgsl,
                    Err(why) => {
                        say(format!("no rims asked: {why}"));
                        return Ok(());
                    }
                };
                let four = Shape {
                    colour: ColourFormat::Rgba16Float,
                    samples: Samples::Four,
                    depth: DepthFormat::Depth32Float,
                };
                let rims = program(wgsl, vec![Read::Depth], Coverage::Alpha, four);
                turn.to(path::<Renderer>(())).ask(&rims, &Making::Rims { mesh, lay, floor: made })?;
                return Ok(());
            }
            Making::Rims { mesh, lay, floor } => (mesh, lay, floor),
        };
        let Self::Making { window } = *self else {
            return Ok(());
        };
        let mut frame = FrameBuilder::on(Ends::Window(window));
        let (floored, under) = frame.draw(Floor, Float, scale::Full, samples::One, depth::Write, ());
        let (rims, _) = frame.draw(Rims, Float, scale::Full, samples::Four, depth::Write, (under,));
        let create = match frame.finish(Lay, Effect::new(&lay, (floored, rims))) {
            Ok(create) => create,
            Err(why) => {
                say(format!("the frame was not built: {why:?}"));
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&create, &MadePrograms { mesh, floor, rims: made })?;

        Ok(())
    }
}

impl Handles<Answer<Framed, MadePrograms>> for Rimmer {
    type Sends = (Asks<Path<Renderer>, Place, Placing>,);

    fn handle(
        &mut self,
        (answered, made): (Answered<Framed>, MadePrograms),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Framed::Made(frame)) = answered else {
            say(format!("the frame was not made: {answered:?}"));
            return Ok(());
        };
        say(format!("made {frame:?}"));
        let Self::Making { window } = *self else {
            return Ok(());
        };
        let join = Join::to::<Floor>(Frame::Main(window), made.floor, Vec::new());
        let place = Place { mesh: made.mesh, instances: Drawn::Count(1), joins: vec![join] };
        turn.to(path::<Renderer>(())).ask(&place, &Placing::Floor { mesh: made.mesh, rims: made.rims })?;

        Ok(())
    }
}

impl Handles<Answer<Placed, Placing>> for Rimmer {
    type Sends = (Asks<Path<Renderer>, Place, Placing>,);

    fn handle(
        &mut self,
        (answered, placing): (Answered<Placed>, Placing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Placed::Made(_)) = answered else {
            say(format!("a placement was not made: {answered:?}"));
            return Ok(());
        };
        let Self::Making { window } = *self else {
            return Ok(());
        };
        let Placing::Floor { mesh, rims } = placing else {
            say(String::from("placed floor rims"));
            *self = Self::Drawing;
            return Ok(());
        };
        let join = Join::to::<Rims>(Frame::Main(window), rims, Vec::new());
        let place = Place { mesh, instances: Drawn::Count(RIMS), joins: vec![join] };
        turn.to(path::<Renderer>(())).ask(&place, &Placing::Rims)?;

        Ok(())
    }
}
