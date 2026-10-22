//! The page's entry points, and an actor that makes a frame of a sky, an effect that reads nothing, a depth pass over
//! it, a pass tested against that, a pattern drawn at half size, and a tint over all of them that lays the pattern
//! over a band of the scene, places a surface and a splat in it, then reads back what was drawn and times the frame's
//! passes.

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, BlobRef, ChildOf, Ending, Fault, Handles, On, Path, Render, Start, Tick,
    Top, Turn, path,
};
use aethereal_chassis_browser::canvas::{self, Density};
use aethereal_chassis_browser::{Browser, Hosted, Listening, Performance, open, renderer};
use aethereal_data::{Bound, Plain};
use aethereal_render::Renderer;
use aethereal_render::effect::{CreateEffect, EffectId, Effected, Read};
use aethereal_render::frame::builder::{Effect, Float, FrameBuilder, depth, scale};
use aethereal_render::frame::{CreateFrame, Ends, Frame, Framed, ShapeOf, Shaped};
use aethereal_render::mesh::{Attribute, CreateMesh, MeshId, Meshed, VertexFormat, VertexLayout};
use aethereal_render::pass::{Main, Pass};
use aethereal_render::placement::{Drawn, Join, Place, Placed};
use aethereal_render::program::{Blend, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId, Programmed};
use aethereal_render::readback::{Pixels, ReadBack, Readable};
use aethereal_render::shape::{ColourFormat, Shape};
use aethereal_render::timing::{TimeFrame, Timed};
use aethereal_substrate::Engine;
use aethereal_window::{Heard, Opening, WindowId, Windowing, Windows};
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
    let mut opening = Opening::new(setup.source::<Heard>(), setup.controller::<Windowing>());
    let (gpu, [shown]) = open([canvas.clone()], &mut opening).await?;
    let (windows, hosting) = opening.config();
    let mut setup = setup
        .add_root::<Windows>(&(), windows)
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

    Ok(Program { browser: Browser::new(engine, Performance, mappings, Hosted::Windows(hosting)), _canvas: listening })
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
#[kind(name = "aether.example.nothing")]
struct Nothing;

/// The framer's config: the window it draws on.
#[derive(Plain)]
#[kind(name = "aether.example.shows")]
struct Shows {
    window: WindowId,
}

/// The pass that begins the scene with a sky, an effect that reads nothing.
#[derive(Plain)]
#[kind(name = "aether.example.sky")]
struct Sky;

impl Pass for Sky {}

/// The pass that draws surfaces over the sky, writing their depth.
#[derive(Plain)]
#[kind(name = "aether.example.surfaces")]
struct Surfaces;

impl Pass for Surfaces {}

/// The pass that draws splats over the scene, tested against the surfaces' depth.
#[derive(Plain)]
#[kind(name = "aether.example.splats")]
struct Splats;

impl Pass for Splats {}

/// The pass that draws a pattern into a target of half the window's size.
#[derive(Plain)]
#[kind(name = "aether.example.detail")]
struct Detail;

impl Pass for Detail {}

/// The pass that tints the scene onto the window.
#[derive(Plain)]
#[kind(name = "aether.example.tint")]
struct Tint;

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

/// A program that draws the quad across the canvas's middle, green, behind the surface: drawn where nothing nearer was.
const SPLAT: &str = "
@vertex
fn place(@location(0) corner: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4(corner.x, corner.y * 0.5, 0.7, 1.0);
}

@fragment
fn shade() -> @location(0) vec4<f32> {
    return vec4(0.0, 1.0, 0.0, 1.0);
}
";

/// An effect that reads nothing and draws a sky, blue at the top of the canvas and half as blue at its bottom, by the
/// place across it the renderer's vertex stage writes.
const SKY: &str = "
@fragment
fn sky(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return vec4(0.2, 0.4, 1.0 - 0.5 * uv.y, 1.0);
}
";

/// An effect that reads nothing and draws a checker of its own target's pixels, each lit pixel beside an unlit one.
const DETAIL: &str = "
@fragment
fn detail(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<u32>(at.xy);
    let lit = f32((pixel.x + pixel.y) % 2u);
    return vec4(lit, lit, lit, 1.0);
}
";

/// An effect that halves the green of each pixel of the scene it reads, and over the top eight rows of its right half
/// lays the detail, read at the same place across it by `uv`: red where the detail is half the scene's size each way,
/// rounded up, and green as the detail's checker is there.
const TINT: &str = "
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var detail: texture_2d<f32>;

@fragment
fn tint(@builtin(position) at: vec4<f32>, @location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(at.xy);
    let tinted = textureLoad(scene, pixel, 0) * vec4(1.0, 0.5, 1.0, 1.0);
    let whole = textureDimensions(scene);
    let part = textureDimensions(detail);
    let halved = all((whole + 1u) / 2u == part);
    let checked = textureLoad(detail, vec2<i32>(uv * vec2<f32>(part)), 0).g;
    let banded = pixel.y < 8 && pixel.x >= i32(whole.x) / 2;
    return select(tinted, vec4(f32(halved), checked, 0.0, 1.0), banded);
}
";

/// The frames drawn before the framer reads one back.
const SETTLED: u32 = 3;

/// The framer: it makes what it draws one ask after another, each answer carrying what was made so far, makes its
/// frame, places a surface and a splat, and once a few frames are drawn reads back a frame and times one.
#[derive(Bound)]
#[kind(name = "aether.example.framer")]
enum Framer {
    /// Its asks are under way, and nothing is placed on `window`.
    Making { window: WindowId },
    /// Both are placed: `drawn` frames have passed since.
    Drawing { drawn: u32 },
    /// It has asked for what it reads back.
    Asked,
}

impl ChildOf for Framer {
    type Parent = Top;
}

/// What was learned before the quad was asked for: the window's colour, which the tint draws into.
#[derive(Plain)]
#[kind(name = "aether.example.learned")]
struct Learned {
    window: ColourFormat,
}

/// Which effect was asked for, with what was made before it.
#[derive(Plain)]
#[kind(name = "aether.example.effecting")]
enum Effecting {
    /// The tint, once the quad was made.
    Tint { mesh: MeshId },
    /// The sky, once the tint was.
    Sky { mesh: MeshId, tint: EffectId<(Float, Float)> },
    /// The detail, once the sky was.
    Detail { mesh: MeshId, tint: EffectId<(Float, Float)>, sky: EffectId<()> },
}

/// Which program was asked for, with what was made before it.
#[derive(Plain)]
#[kind(name = "aether.example.making")]
enum Making {
    /// The surface, once the effects were made.
    Surface { mesh: MeshId, effects: Effects },
    /// The splat, once the surface was.
    Splat { mesh: MeshId, effects: Effects, surface: ProgramId },
}

/// The frame's effects.
#[derive(Plain)]
#[kind(name = "aether.example.effects")]
struct Effects {
    tint: EffectId<(Float, Float)>,
    sky: EffectId<()>,
    detail: EffectId<()>,
}

/// What the placements draw with, kept until the frame is made.
#[derive(Plain)]
#[kind(name = "aether.example.made_programs")]
struct MadePrograms {
    mesh: MeshId,
    surface: ProgramId,
    splat: ProgramId,
}

/// Which placement was asked for.
#[derive(Plain)]
#[kind(name = "aether.example.placing")]
enum Placing {
    /// The surface, with what the splat is then placed with.
    Surface { mesh: MeshId, splat: ProgramId },
    /// The splat.
    Splat,
}

impl Actor for Framer {
    type Config = Shows;
    type Key = ();
    type Handles = (
        On<Start>,
        On<Tick>,
        Answer<Shaped, Nothing>,
        Answer<Meshed, Learned>,
        Answer<Effected, Effecting>,
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
    type Sends = (Asks<Path<Renderer>, CreateEffect, Effecting>,);

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
            reads: vec![Read::Float, Read::Float],
            bindings: Vec::new(),
            blend: Blend::Opaque,
            shape: Shape::flat(learned.window),
        };
        turn.to(path::<Renderer>(())).ask(&tint, &Effecting::Tint { mesh })?;

        Ok(())
    }
}

impl Handles<Answer<Effected, Effecting>> for Framer {
    type Sends = (Asks<Path<Renderer>, CreateEffect, Effecting>, Asks<Path<Renderer>, CreateProgram, Making>);

    fn handle(
        &mut self,
        (answered, effecting): (Answered<Effected>, Effecting),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Effected::Made(effect)) = answered else {
            say(format!("an effect was not made: {answered:?}"));
            return Ok(());
        };
        let (mesh, effects) = match effecting {
            Effecting::Tint { mesh } => {
                let Some(tint) = EffectId::of(&effect) else {
                    say(format!("the tint reads what it was not asked to: {:?}", effect.reads()));
                    return Ok(());
                };
                let wgsl = match turn.put(Vec::from(SKY)) {
                    Ok(wgsl) => wgsl,
                    Err(why) => {
                        say(format!("no sky asked: {why}"));
                        return Ok(());
                    }
                };
                let sky = CreateEffect {
                    wgsl,
                    fragment: String::from("sky"),
                    reads: Vec::new(),
                    bindings: Vec::new(),
                    blend: Blend::Opaque,
                    shape: Shape::flat(ColourFormat::Rgba16Float),
                };
                turn.to(path::<Renderer>(())).ask(&sky, &Effecting::Sky { mesh, tint })?;
                return Ok(());
            }
            Effecting::Sky { mesh, tint } => {
                let Some(sky) = EffectId::of(&effect) else {
                    say(format!("the sky reads what it was not asked to: {:?}", effect.reads()));
                    return Ok(());
                };
                let wgsl = match turn.put(Vec::from(DETAIL)) {
                    Ok(wgsl) => wgsl,
                    Err(why) => {
                        say(format!("no detail asked: {why}"));
                        return Ok(());
                    }
                };
                let detail = CreateEffect {
                    wgsl,
                    fragment: String::from("detail"),
                    reads: Vec::new(),
                    bindings: Vec::new(),
                    blend: Blend::Opaque,
                    shape: Shape::flat(ColourFormat::Rgba16Float),
                };
                turn.to(path::<Renderer>(())).ask(&detail, &Effecting::Detail { mesh, tint, sky })?;
                return Ok(());
            }
            Effecting::Detail { mesh, tint, sky } => {
                let Some(detail) = EffectId::of(&effect) else {
                    say(format!("the detail reads what it was not asked to: {:?}", effect.reads()));
                    return Ok(());
                };
                (mesh, Effects { tint, sky, detail })
            }
        };
        let wgsl = match turn.put(Vec::from(SURFACE)) {
            Ok(wgsl) => wgsl,
            Err(why) => {
                say(format!("no surface asked: {why}"));
                return Ok(());
            }
        };
        let surface = program(wgsl, Depth::TestWrite);
        turn.to(path::<Renderer>(())).ask(&surface, &Making::Surface { mesh, effects })?;

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
        let (mesh, effects, surface) = match making {
            Making::Surface { mesh, effects } => {
                let wgsl = match turn.put(Vec::from(SPLAT)) {
                    Ok(wgsl) => wgsl,
                    Err(why) => {
                        say(format!("no splat asked: {why}"));
                        return Ok(());
                    }
                };
                let splat = program(wgsl, Depth::Test);
                turn.to(path::<Renderer>(())).ask(&splat, &Making::Splat { mesh, effects, surface: made })?;
                return Ok(());
            }
            Making::Splat { mesh, effects, surface } => (mesh, effects, surface),
        };
        let Self::Making { window } = *self else {
            return Ok(());
        };
        let mut frame = FrameBuilder::on(Ends::Window(window));
        let sky = frame.effect(Sky, Effect::new(&effects.sky, ()), Float, scale::Full);
        let (scene, depth) = frame.draw_over(Surfaces, sky, depth::Write);
        let scene = frame.draw_over(Splats, scene, depth::Test(&depth));
        let detail = frame.effect(Detail, Effect::new(&effects.detail, ()), Float, scale::Half);
        let create = match frame.finish(Tint, Effect::new(&effects.tint, (scene, detail))) {
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
        let sky_top = pixel(read, width, 2, 2);
        let sky_bottom = pixel(read, width, 2, height - 3);
        say(format!(
            "frame width={width} height={height} format={format:?} surface={surface} splat={splat} sky_top={sky_top} \
             sky_bottom={sky_bottom}"
        ));
        // A block of two by two pixels of the band, from an even place: one pixel of the half-size detail each, so all
        // four are alike, and unlike the block beside them. Drawn at the window's size, each would be unlike the next.
        let (x, y) = ((width / 2 + 2) & !1, 2);
        let block = [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1), (x + 2, y)].map(|(x, y)| pixel(read, width, x, y));
        say(format!("detail block={} beside={}", block[..4].join("/"), block[4]));

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
