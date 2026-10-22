//! The page's entry points, and an actor that makes a frame of a depth pass, a pass tested against it, and a tint over
//! both, places a surface and a splat in it, then reads back what was drawn and times the frame's passes.

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, BlobRef, ChildOf, Ending, Fault, Handles, On, Path, Render, Start, Tick,
    Top, Turn, path,
};
use aethereal_chassis_browser::canvas::{self, Density};
use aethereal_chassis_browser::{Browser, Listening, Performance, open, renderer};
use aethereal_data::{Bound, Kind, KindName, Plain};
use aethereal_render::Renderer;
use aethereal_render::effect::{CreateEffect, EffectId, Effected, Read};
use aethereal_render::frame::builder::{Effect, Float, FrameBuilder, depth};
use aethereal_render::frame::{CreateFrame, Ends, Frame, Framed, ShapeOf, Shaped};
use aethereal_render::mesh::{Attribute, CreateMesh, MeshId, Meshed, VertexFormat, VertexLayout};
use aethereal_render::pass::{Main, Pass};
use aethereal_render::placement::{Drawn, Join, Place, Placed};
use aethereal_render::program::{Blend, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId, Programmed};
use aethereal_render::readback::{Pixels, ReadBack, Readable};
use aethereal_render::shape::{ColourFormat, Shape};
use aethereal_render::timing::{TimeFrame, Timed};
use aethereal_substrate::Engine;
use aethereal_window::{Heard, Opening, WindowId, Windows};
use core::cell::RefCell;
use core::mem;
use core::num::NonZeroUsize;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::wasm_bindgen;
use web_sys::HtmlCanvasElement;

thread_local! {
    /// What the framer was answered, for the page to show.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Notes what the framer was answered.
fn say(line: String) {
    SAID.with_borrow_mut(|said| said.push(line));
}

/// The program the page runs, for its `requestAnimationFrame` to call, and the page's listeners, kept while it runs.
#[wasm_bindgen]
pub struct Program {
    browser: Browser<Performance>,
    _canvas: Listening,
}

/// Opens `canvas` as the one window and WebGPU on it, then makes the engine with the window root, the renderer, handed
/// the GPU, and the framer as its roots, and listens to the page for the canvas's size. Its blobs hold a frame of a
/// large canvas.
///
/// # Errors
///
/// When WebGPU will not open on the canvas, a root does not begin, or the page will not be listened to.
#[wasm_bindgen]
pub async fn start(canvas: HtmlCanvasElement) -> Result<Program, JsError> {
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 1 << 20, 1 << 26, 0)
        .with_cycle::<(Tick, Render)>()
        .map_err(|unordered| JsError::new(&format!("the cycle is out of order: {unordered:?}")))?;
    let mut opening = Opening::new(setup.source::<Heard>());
    let (gpu, shown) = open(canvas.clone(), &mut opening).await?;
    let mut setup = setup
        .add_root::<Windows>(&(), opening.config())
        .map_err(|refused| JsError::new(&format!("the window root did not begin: {refused:?}")))?;
    let (rendering, mappings) = renderer(&mut setup, gpu);
    let engine = setup
        .add_root::<Renderer>(&(), rendering)
        .map_err(|refused| JsError::new(&format!("the renderer did not begin: {refused:?}")))?
        .add_root::<Framer>(&(), Shows { window: shown.window() })
        .map_err(|refused| JsError::new(&format!("the framer did not begin: {refused:?}")))?
        .ready();

    // The canvas is drawn at every device pixel of the size the page lays it out at, followed as it is resized.
    let listening = canvas::listen(&canvas, &shown, Density::SCREEN)?;

    Ok(Program { browser: Browser::new(engine, Performance, mappings), _canvas: listening })
}

#[wasm_bindgen]
impl Program {
    /// Runs one frame, at the timestamp `requestAnimationFrame` handed its callback, in milliseconds.
    pub fn frame(&mut self, timestamp_millis: f64) {
        self.browser.frame(timestamp_millis);
    }

    /// What the framer was answered since this was last asked, a line each.
    #[must_use]
    #[expect(clippy::unused_self, reason = "the page asks it of the program it started")]
    pub fn said(&self) -> Vec<String> {
        SAID.with_borrow_mut(mem::take)
    }
}

/// The context of an ask that needs none.
#[derive(Plain)]
struct Nothing;

impl Kind for Nothing {
    const NAME: KindName = KindName::new("aether.example.nothing");
}

/// The framer's config: the window it draws on.
#[derive(Plain)]
struct Shows {
    window: WindowId,
}

impl Kind for Shows {
    const NAME: KindName = KindName::new("aether.example.shows");
}

/// The pass that draws surfaces into the scene, writing their depth.
#[derive(Plain)]
struct Surfaces;

impl Kind for Surfaces {
    const NAME: KindName = KindName::new("aether.example.surfaces");
}

impl Pass for Surfaces {}

/// The pass that draws splats over the scene, tested against the surfaces' depth.
#[derive(Plain)]
struct Splats;

impl Kind for Splats {
    const NAME: KindName = KindName::new("aether.example.splats");
}

impl Pass for Splats {}

/// The pass that tints the scene onto the window.
#[derive(Plain)]
struct Tint;

impl Kind for Tint {
    const NAME: KindName = KindName::new("aether.example.tint");
}

impl Pass for Tint {}

/// A program that draws the quad over the left half of the canvas's middle, red, half way into the depth, writing it.
const SURFACE: &str = "
@vertex
fn place(@location(0) corner: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4(corner.x * 0.5 - 0.5, corner.y * 0.5, 0.5, 1.0);
}

@fragment
fn shade() -> @location(0) vec4<f32> {
    return vec4(1.0, 0.0, 0.0, 1.0);
}
";

/// A program that draws the quad over the whole canvas, green, behind the surface: drawn where nothing nearer was.
const SPLAT: &str = "
@vertex
fn place(@location(0) corner: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4(corner, 0.7, 1.0);
}

@fragment
fn shade() -> @location(0) vec4<f32> {
    return vec4(0.0, 1.0, 0.0, 1.0);
}
";

/// An effect that halves the green of each pixel of the scene it reads.
const TINT: &str = "
@group(0) @binding(0) var scene: texture_2d<f32>;

@fragment
fn tint(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(scene, vec2<i32>(at.xy), 0) * vec4(1.0, 0.5, 1.0, 1.0);
}
";

/// The frames drawn before the framer reads one back.
const SETTLED: u32 = 3;

/// The framer: it makes what it draws one ask after another, each answer carrying what was made so far, makes its
/// frame, places a surface and a splat, and once a few frames are drawn reads back a frame and times one.
#[derive(Bound)]
enum Framer {
    /// Its asks are under way, and nothing is placed on `window`.
    Making { window: WindowId },
    /// Both are placed: `drawn` frames have passed since.
    Drawing { drawn: u32 },
    /// It has asked for what it reads back.
    Asked,
}

impl Kind for Framer {
    const NAME: KindName = KindName::new("aether.example.framer");
}

impl ChildOf for Framer {
    type Parent = Top;
}

/// What was learned before the quad was asked for: the window's colour, which the tint draws into.
#[derive(Plain)]
struct Learned {
    window: ColourFormat,
}

impl Kind for Learned {
    const NAME: KindName = KindName::new("aether.example.learned");
}

/// What was made before the tint was asked for.
#[derive(Plain)]
struct MadeMesh {
    mesh: MeshId,
}

impl Kind for MadeMesh {
    const NAME: KindName = KindName::new("aether.example.made_mesh");
}

/// Which program was asked for, with what was made before it.
#[derive(Plain)]
enum Making {
    /// The surface, once the tint was made.
    Surface { mesh: MeshId, tint: EffectId<(Float,)> },
    /// The splat, once the surface was.
    Splat { mesh: MeshId, tint: EffectId<(Float,)>, surface: ProgramId },
}

impl Kind for Making {
    const NAME: KindName = KindName::new("aether.example.making");
}

/// What the placements draw with, kept until the frame is made.
#[derive(Plain)]
struct MadePrograms {
    mesh: MeshId,
    surface: ProgramId,
    splat: ProgramId,
}

impl Kind for MadePrograms {
    const NAME: KindName = KindName::new("aether.example.made_programs");
}

/// Which placement was asked for.
#[derive(Plain)]
enum Placing {
    /// The surface, with what the splat is then placed with.
    Surface { mesh: MeshId, splat: ProgramId },
    /// The splat.
    Splat,
}

impl Kind for Placing {
    const NAME: KindName = KindName::new("aether.example.placing");
}

impl Actor for Framer {
    type Config = Shows;
    type Key = ();
    type Handles = (
        On<Start>,
        On<Tick>,
        Answer<Shaped, Nothing>,
        Answer<Meshed, Learned>,
        Answer<Effected, MadeMesh>,
        Answer<Programmed, Making>,
        Answer<Framed, MadePrograms>,
        Answer<Placed, Placing>,
        Answer<Pixels, Nothing>,
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

/// How the quad's corners are laid out: a place of two floats.
fn corners() -> VertexLayout {
    let corner = Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 };

    VertexLayout { stride: 8, attributes: vec![corner] }
}

/// A program of the WGSL the blob `wgsl` holds over the quad, drawing into the scene, a target of floats with depth,
/// using depth as `depth` says.
fn program(wgsl: BlobRef, depth: Depth) -> CreateProgram {
    CreateProgram {
        wgsl,
        vertex: String::from("place"),
        fragment: String::from("shade"),
        layout: corners(),
        instances: PerInstance::None,
        bindings: Vec::new(),
        state: Fixed { blend: Blend::Opaque, depth, cull: Cull::None },
        shape: Shape::with_depth(ColourFormat::Rgba16Float),
    }
}

impl Handles<On<Start>> for Framer {
    type Sends = (Asks<Path<Renderer>, ShapeOf, Nothing>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Self::Making { window } = *self else {
            return Ok(());
        };
        turn.to(path::<Renderer>(())).ask(&ShapeOf::pass::<Main>(Frame::Main(window)), &Nothing)?;

        Ok(())
    }
}

impl Handles<Answer<Shaped, Nothing>> for Framer {
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

impl Handles<Answer<Meshed, Learned>> for Framer {
    type Sends = (Asks<Path<Renderer>, CreateEffect, MadeMesh>,);

    fn handle(
        &mut self,
        (answered, learned): (Answered<Meshed>, Learned),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Meshed::Made(mesh)) = answered else {
            say(format!("the quad was not made: {answered:?}"));
            return Ok(());
        };
        let wgsl = match turn.put(Vec::from(TINT)) {
            Ok(wgsl) => wgsl,
            Err(why) => {
                say(format!("no tint asked: {why}"));
                return Ok(());
            }
        };
        let tint = CreateEffect {
            wgsl,
            fragment: String::from("tint"),
            reads: vec![Read::Float],
            bindings: Vec::new(),
            blend: Blend::Opaque,
            shape: Shape::flat(learned.window),
        };
        turn.to(path::<Renderer>(())).ask(&tint, &MadeMesh { mesh })?;

        Ok(())
    }
}

impl Handles<Answer<Effected, MadeMesh>> for Framer {
    type Sends = (Asks<Path<Renderer>, CreateProgram, Making>,);

    fn handle(
        &mut self,
        (answered, made): (Answered<Effected>, MadeMesh),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Effected::Made(effect)) = answered else {
            say(format!("the tint was not made: {answered:?}"));
            return Ok(());
        };
        let Some(tint) = EffectId::of(&effect) else {
            say(format!("the tint reads what it was not asked to: {:?}", effect.reads()));
            return Ok(());
        };
        let wgsl = match turn.put(Vec::from(SURFACE)) {
            Ok(wgsl) => wgsl,
            Err(why) => {
                say(format!("no surface asked: {why}"));
                return Ok(());
            }
        };
        let surface = program(wgsl, Depth::TestWrite);
        turn.to(path::<Renderer>(())).ask(&surface, &Making::Surface { mesh: made.mesh, tint })?;

        Ok(())
    }
}

impl Handles<Answer<Programmed, Making>> for Framer {
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
        let (mesh, tint, surface) = match making {
            Making::Surface { mesh, tint } => {
                let wgsl = match turn.put(Vec::from(SPLAT)) {
                    Ok(wgsl) => wgsl,
                    Err(why) => {
                        say(format!("no splat asked: {why}"));
                        return Ok(());
                    }
                };
                let splat = program(wgsl, Depth::Test);
                turn.to(path::<Renderer>(())).ask(&splat, &Making::Splat { mesh, tint, surface: made })?;
                return Ok(());
            }
            Making::Splat { mesh, tint, surface } => (mesh, tint, surface),
        };
        let Self::Making { window } = *self else {
            return Ok(());
        };
        let mut frame = FrameBuilder::on(Ends::Window(window));
        let (scene, depth) = frame.draw(Surfaces, Float, depth::Write);
        let scene = frame.draw_over(Splats, scene, depth::Test(&depth));
        let create = match frame.finish(Tint, Effect::new(&tint, (scene,))) {
            Ok(create) => create,
            Err(why) => {
                say(format!("the frame was not built: {why:?}"));
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&create, &MadePrograms { mesh, surface, splat: made })?;

        Ok(())
    }
}

impl Handles<Answer<Framed, MadePrograms>> for Framer {
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
        let join = Join::to::<Surfaces>(Frame::Main(window), made.surface, Vec::new());
        let place = Place { mesh: made.mesh, instances: Drawn::Count(1), joins: vec![join] };
        turn.to(path::<Renderer>(())).ask(&place, &Placing::Surface { mesh: made.mesh, splat: made.splat })?;

        Ok(())
    }
}

impl Handles<Answer<Placed, Placing>> for Framer {
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
        let Placing::Surface { mesh, splat } = placing else {
            say(String::from("placed surface splat"));
            *self = Self::Drawing { drawn: 0 };
            return Ok(());
        };
        let join = Join::to::<Splats>(Frame::Main(window), splat, Vec::new());
        let place = Place { mesh, instances: Drawn::Count(1), joins: vec![join] };
        turn.to(path::<Renderer>(())).ask(&place, &Placing::Splat)?;

        Ok(())
    }
}

impl Handles<On<Tick>> for Framer {
    type Sends = (Asks<Path<Renderer>, ReadBack, Nothing>, Asks<Path<Renderer>, TimeFrame, Nothing>);

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Self::Drawing { drawn } = *self else {
            return Ok(());
        };
        if drawn < SETTLED {
            *self = Self::Drawing { drawn: drawn + 1 };
            return Ok(());
        }
        turn.to(path::<Renderer>(())).ask(&ReadBack { of: Readable::Frame }, &Nothing)?.ask(&TimeFrame, &Nothing)?;
        *self = Self::Asked;

        Ok(())
    }
}

/// The pixel at `x`, `y` of `pixels`, `width` across, four bytes each.
fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> String {
    let at = ((y * width + x) * 4) as usize;
    let channels = pixels.get(at..at + 4).unwrap_or_default();

    channels.iter().map(u8::to_string).collect::<Vec<_>>().join(",")
}

impl Handles<Answer<Pixels, Nothing>> for Framer {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, _): (Answered<Pixels>, Nothing),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Pixels::Read { width, height, format, pixels }) = answered else {
            say(format!("nothing was read: {answered:?}"));
            return Ok(());
        };
        let read = turn.read(&pixels);
        let surface = pixel(read, width, width / 4, height / 2);
        let splat = pixel(read, width, width * 3 / 4, height / 2);
        let corner = pixel(read, width, 2, 2);
        say(format!(
            "frame width={width} height={height} format={format:?} surface={surface} splat={splat} corner={corner}"
        ));

        Ok(())
    }
}

impl Handles<Answer<Timed, Nothing>> for Framer {
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
