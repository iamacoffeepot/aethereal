//! The page's entry points, an actor that draws a map into a texture frame and shows it in the window's main frame it
//! makes, an actor that asks to replace that main frame, and an actor that keeps a glow past one in a frame that ends
//! in floats and darkens it into another.

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, BlobRef, ChildOf, Ending, Fault, Handles, On, Path, Render, Start, Tells,
    Tick, Top, Turn, path,
};
use aethereal_chassis_browser::{Browser, Performance, open, renderer};
use aethereal_data::{Bound, Plain};
use aethereal_render::effect::{CreateEffect, EffectId, Effected};
use aethereal_render::frame::builder::{Effect, FrameBuilder, Onto, depth};
use aethereal_render::frame::{CreateFrame, Ends, Frame, FrameId, FrameRefused, Framed, ShapeOf, Shaped, Size};
use aethereal_render::mesh::{Attribute, CreateMesh, MeshId, Meshed, VertexFormat, VertexLayout};
use aethereal_render::pass::{Main, Pass};
use aethereal_render::placement::{Bind, Drawn, Filter, Join, Place, Placed, Resource, Sampling, Wrap};
use aethereal_render::program::{
    Binding, BindingKind, Blend, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId, Programmed,
};
use aethereal_render::readback::{Pixels, ReadBack, Readable};
use aethereal_render::shape::{ColourFormat, Shape};
use aethereal_render::texture::{TextureFormat, TextureId};
use aethereal_render::{Release, Renderer};
use aethereal_substrate::Engine;
use aethereal_window::{Heard, Opening, WindowId, Windows};
use core::cell::RefCell;
use core::mem;
use core::num::NonZeroUsize;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::wasm_bindgen;
use web_sys::HtmlCanvasElement;

thread_local! {
    /// What the actors were answered, for the page to show.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Notes what an actor was answered.
fn say(line: String) {
    SAID.with_borrow_mut(|said| said.push(line));
}

/// The program the page runs, for its `requestAnimationFrame` to call.
#[wasm_bindgen]
pub struct Program {
    browser: Browser<Performance>,
}

/// Opens `canvas` as the one window and WebGPU on it, then makes the engine with the window root, the renderer, handed
/// the GPU, the claimer, the mapper, and the glower as its roots. Its blobs hold a frame of a large canvas.
///
/// # Errors
///
/// When WebGPU will not open on the canvas, or a root does not begin.
#[wasm_bindgen]
pub async fn start(canvas: HtmlCanvasElement) -> Result<Program, JsError> {
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 1 << 20, 1 << 26, 0)
        .with_cycle::<(Tick, Render)>()
        .map_err(|unordered| JsError::new(&format!("the cycle is out of order: {unordered:?}")))?;
    let mut opening = Opening::new(setup.source::<Heard>());
    let (gpu, [shown]) = open([canvas], &mut opening).await?;
    let mut setup = setup
        .add_root::<Windows>(&(), opening.config())
        .map_err(|refused| JsError::new(&format!("the window root did not begin: {refused:?}")))?;
    let (rendering, mappings) = renderer(&mut setup, gpu);
    // The mapper reaches the claimer at its typed path, so the claimer is made first.
    let engine = setup
        .add_root::<Renderer>(&(), rendering)
        .map_err(|refused| JsError::new(&format!("the renderer did not begin: {refused:?}")))?
        .add_root::<Claimer>(&(), Nothing)
        .map_err(|refused| JsError::new(&format!("the claimer did not begin: {refused:?}")))?
        .add_root::<Mapper>(&(), Shows { window: shown.window() })
        .map_err(|refused| JsError::new(&format!("the mapper did not begin: {refused:?}")))?
        .add_root::<Glower>(&(), Nothing)
        .map_err(|refused| JsError::new(&format!("the glower did not begin: {refused:?}")))?
        .ready();

    Ok(Program { browser: Browser::new(engine, Performance, mappings) })
}

#[wasm_bindgen]
impl Program {
    /// Runs one frame, at the timestamp `requestAnimationFrame` handed its callback, in milliseconds.
    pub fn frame(&mut self, timestamp_millis: f64) {
        self.browser.frame(timestamp_millis);
    }

    /// What the actors were answered since this was last asked, a line each.
    #[must_use]
    #[expect(clippy::unused_self, reason = "the page asks it of the program it started")]
    pub fn said(&self) -> Vec<String> {
        SAID.with_borrow_mut(mem::take)
    }
}

/// An empty config, and the context of an ask that needs none.
#[derive(Plain)]
#[kind(name = "aether.example.nothing")]
struct Nothing;

/// The mapper's config: the window it draws on.
#[derive(Plain)]
#[kind(name = "aether.example.shows")]
struct Shows {
    window: WindowId,
}

/// The one pass of the map's frame.
#[derive(Plain)]
#[kind(name = "aether.example.mapping")]
struct Mapping;

impl Pass for Mapping {}

/// A program that fills the quad over the whole map, red on its left half and blue on its right.
const MAP: &str = "
@vertex
fn place(@location(0) corner: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4(corner, 0.0, 1.0);
}

@fragment
fn shade(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    return select(vec4(0.0, 0.0, 1.0, 1.0), vec4(1.0, 0.0, 0.0, 1.0), at.x < 32.0);
}
";

/// A program that draws the quad over the middle half of the canvas, sampling the map across it.
const SHOW: &str = "
@group(0) @binding(0) var map: texture_2d<f32>;
@group(0) @binding(1) var soft: sampler;

struct Between {
    @builtin(position) at: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn place(@location(0) corner: vec2<f32>) -> Between {
    return Between(vec4(corner * 0.5, 0.0, 1.0), vec2(corner.x * 0.5 + 0.5, 0.5 - corner.y * 0.5));
}

@fragment
fn shade(between: Between) -> @location(0) vec4<f32> {
    return textureSample(map, soft, between.uv);
}
";

/// The map's size: a side of 64 pixels.
const MAP_SIZE: Size = Size { width: 64, height: 64 };

/// The frames drawn before the mapper reads one back, and between releasing the map and reading again.
const SETTLED: u32 = 3;

/// The mapper: it makes what it draws one ask after another, each answer carrying what was made so far: the map's
/// frame, a placement in it, the window's main frame, and the quad that shows the map in it. Once a few frames are
/// drawn it reads back the window and the map, releases the map's frame, and reads the window back again.
#[derive(Bound)]
#[kind(name = "aether.example.mapper")]
enum Mapper {
    /// Its asks are under way, and nothing is placed on `window`.
    Making { window: WindowId },
    /// The quad is placed: `drawn` frames have passed since.
    Drawing { map: FrameId, texture: TextureId, drawn: u32 },
    /// The map's frame is released: `drawn` frames have passed since.
    Released { texture: TextureId, drawn: u32 },
    /// It has read back all it reads.
    Done,
}

impl ChildOf for Mapper {
    type Parent = Top;
}

/// What was learned before the quad was asked for: the window's colour, which the quad is drawn in.
#[derive(Plain)]
#[kind(name = "aether.example.learned")]
struct Learned {
    window: ColourFormat,
}

/// Which program was asked for, with what was made before it.
#[derive(Plain)]
#[kind(name = "aether.example.making")]
enum Making {
    /// The map's, once the quad was made.
    Map { window: ColourFormat, mesh: MeshId },
    /// The quad's, once the map's was.
    Show { mesh: MeshId, map: ProgramId },
}

/// Which frame was asked for, with what was made before it.
#[derive(Plain)]
#[kind(name = "aether.example.framing")]
enum Framing {
    /// The map's, once both programs were made.
    Map { mesh: MeshId, map: ProgramId, show: ProgramId },
    /// The window's, once the map was placed in its frame.
    Main { mesh: MeshId, show: ProgramId, map: FrameId, texture: TextureId },
}

/// Which placement was asked for, with what was made before it.
#[derive(Plain)]
#[kind(name = "aether.example.placing")]
enum Placing {
    /// The map's, in its frame.
    Map { mesh: MeshId, show: ProgramId, map: FrameId, texture: TextureId },
    /// The quad that shows it, in the window's.
    Show { map: FrameId, texture: TextureId },
}

/// What was asked to be read back.
#[derive(Plain)]
#[kind(name = "aether.example.of")]
enum Of {
    /// The window, with the map's frame still made.
    Shown,
    /// The map.
    Map,
    /// The window, the map's frame released.
    Kept,
    /// The map, its frame released.
    Released,
}

impl Actor for Mapper {
    type Config = Shows;
    type Key = ();
    type Handles = (
        On<Start>,
        On<Tick>,
        Answer<Shaped, Nothing>,
        Answer<Meshed, Learned>,
        Answer<Programmed, Making>,
        Answer<Framed, Framing>,
        Answer<Placed, Placing>,
        Answer<Pixels, Of>,
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

/// A program of the WGSL the blob `wgsl` holds over the quad, drawing into `shape`, binding `bindings`.
fn program(wgsl: BlobRef, shape: Shape, bindings: Vec<Binding>) -> CreateProgram {
    CreateProgram {
        wgsl,
        vertex: String::from("place"),
        fragment: String::from("shade"),
        layout: corners(),
        instances: PerInstance::None,
        bindings,
        state: Fixed { blend: Blend::Opaque, depth: Depth::Off, cull: Cull::None },
        shape,
    }
}

/// The window's main frame: one pass, [`Main`], onto it, writing depth, as the renderer's own is.
fn main_frame(window: WindowId) -> Result<CreateFrame, FrameRefused> {
    FrameBuilder::on(Ends::Window(window)).finish(Main, Onto(depth::Write))
}

impl Handles<On<Start>> for Mapper {
    type Sends = (Asks<Path<Renderer>, ShapeOf, Nothing>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Self::Making { window } = *self else {
            return Ok(());
        };
        turn.to(path::<Renderer>(())).ask(&ShapeOf::pass::<Main>(Frame::Main(window)), &Nothing)?;

        Ok(())
    }
}

impl Handles<Answer<Shaped, Nothing>> for Mapper {
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

impl Handles<Answer<Meshed, Learned>> for Mapper {
    type Sends = (Asks<Path<Renderer>, CreateProgram, Making>,);

    fn handle(
        &mut self,
        (answered, learned): (Answered<Meshed>, Learned),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Meshed::Made(mesh)) = answered else {
            say(format!("the quad was not made: {answered:?}"));
            return Ok(());
        };
        let wgsl = match turn.put(Vec::from(MAP)) {
            Ok(wgsl) => wgsl,
            Err(why) => {
                say(format!("no map asked: {why}"));
                return Ok(());
            }
        };
        let map = program(wgsl, Shape::flat(ColourFormat::Rgba8), Vec::new());
        turn.to(path::<Renderer>(())).ask(&map, &Making::Map { window: learned.window, mesh })?;

        Ok(())
    }
}

impl Handles<Answer<Programmed, Making>> for Mapper {
    type Sends = (Asks<Path<Renderer>, CreateProgram, Making>, Asks<Path<Renderer>, CreateFrame, Framing>);

    fn handle(
        &mut self,
        (answered, making): (Answered<Programmed>, Making),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Programmed::Made(made)) = answered else {
            say(format!("a program was not made: {answered:?}"));
            return Ok(());
        };
        let (mesh, map) = match making {
            Making::Map { window, mesh } => {
                let bindings = vec![
                    Binding { group: 0, binding: 0, kind: BindingKind::Texture },
                    Binding { group: 0, binding: 1, kind: BindingKind::Sampler },
                ];
                let wgsl = match turn.put(Vec::from(SHOW)) {
                    Ok(wgsl) => wgsl,
                    Err(why) => {
                        say(format!("no show asked: {why}"));
                        return Ok(());
                    }
                };
                let show = program(wgsl, Shape::with_depth(window), bindings);
                turn.to(path::<Renderer>(())).ask(&show, &Making::Show { mesh, map: made })?;
                return Ok(());
            }
            Making::Show { mesh, map } => (mesh, map),
        };
        let create = match FrameBuilder::on(Ends::Texture { size: MAP_SIZE, format: TextureFormat::Rgba8 })
            .finish(Mapping, Onto(depth::Off))
        {
            Ok(create) => create,
            Err(why) => {
                say(format!("the map's frame was not built: {why:?}"));
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&create, &Framing::Map { mesh, map, show: made })?;

        Ok(())
    }
}

impl Handles<Answer<Framed, Framing>> for Mapper {
    type Sends = (Asks<Path<Renderer>, Place, Placing>, Tells<Path<Claimer>, Claim>);

    fn handle(
        &mut self,
        (answered, framing): (Answered<Framed>, Framing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Self::Making { window } = *self else {
            return Ok(());
        };
        match (answered, framing) {
            (Answered::Yes(Framed::Texture { frame, texture }), Framing::Map { mesh, map, show }) => {
                say(format!("made map {frame:?} {texture:?}"));
                let join = Join::to::<Mapping>(Frame::Texture(frame), map, Vec::new());
                let place = Place { mesh, instances: Drawn::Count(1), joins: vec![join] };
                turn.to(path::<Renderer>(())).ask(&place, &Placing::Map { mesh, show, map: frame, texture })?;
            }
            (Answered::Yes(Framed::Made(main)), Framing::Main { mesh, show, map, texture }) => {
                say(format!("made main {main:?}"));
                let sampling = Sampling { filter: Filter::Nearest, wrap: Wrap::Clamp };
                let bindings = vec![
                    Bind { group: 0, binding: 0, to: Resource::Texture(texture) },
                    Bind { group: 0, binding: 1, to: Resource::Sampler(sampling) },
                ];
                let join = Join::to::<Main>(Frame::Main(window), show, bindings);
                let place = Place { mesh, instances: Drawn::Count(1), joins: vec![join] };
                turn.to(path::<Renderer>(())).ask(&place, &Placing::Show { map, texture })?;
                turn.to(path::<Claimer>(())).send(&Claim { window })?;
            }
            (answered, _) => say(format!("a frame was not made: {answered:?}")),
        }

        Ok(())
    }
}

impl Handles<Answer<Placed, Placing>> for Mapper {
    type Sends = (Asks<Path<Renderer>, CreateFrame, Framing>,);

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
        let (mesh, show, map, texture) = match placing {
            Placing::Map { mesh, show, map, texture } => (mesh, show, map, texture),
            Placing::Show { map, texture } => {
                say(String::from("placed map show"));
                *self = Self::Drawing { map, texture, drawn: 0 };
                return Ok(());
            }
        };
        let create = match main_frame(window) {
            Ok(create) => create,
            Err(why) => {
                say(format!("the main frame was not built: {why:?}"));
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&create, &Framing::Main { mesh, show, map, texture })?;

        Ok(())
    }
}

impl Handles<On<Tick>> for Mapper {
    type Sends = (Asks<Path<Renderer>, ReadBack, Of>, Tells<Path<Renderer>, Release>);

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        match *self {
            Self::Drawing { map, texture, drawn } if drawn < SETTLED => {
                *self = Self::Drawing { map, texture, drawn: drawn + 1 };
            }
            Self::Drawing { map, texture, .. } => {
                turn.to(path::<Renderer>(()))
                    .ask(&ReadBack { of: Readable::Frame }, &Of::Shown)?
                    .ask(&ReadBack { of: Readable::Texture(texture) }, &Of::Map)?
                    .send(&Release::Frame(map))?;
                *self = Self::Released { texture, drawn: 0 };
            }
            Self::Released { texture, drawn } if drawn < SETTLED => {
                *self = Self::Released { texture, drawn: drawn + 1 };
            }
            Self::Released { texture, .. } => {
                turn.to(path::<Renderer>(()))
                    .ask(&ReadBack { of: Readable::Frame }, &Of::Kept)?
                    .ask(&ReadBack { of: Readable::Texture(texture) }, &Of::Released)?;
                *self = Self::Done;
            }
            Self::Making { .. } | Self::Done => {}
        }

        Ok(())
    }
}

/// The pixel at `x`, `y` of `pixels`, `width` across, four bytes each.
fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> String {
    let at = ((y * width + x) * 4) as usize;
    let channels = pixels.get(at..at + 4).unwrap_or_default();

    channels.iter().map(u8::to_string).collect::<Vec<_>>().join(",")
}

impl Handles<Answer<Pixels, Of>> for Mapper {
    type Sends = ();

    fn handle(&mut self, (answered, of): (Answered<Pixels>, Of), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let Answered::Yes(Pixels::Read { width, height, pixels, .. }) = answered else {
            let what = match of {
                Of::Shown | Of::Kept => "frame",
                Of::Map | Of::Released => "map",
            };
            say(format!("unread {what}={answered:?}"));
            return Ok(());
        };
        let read = turn.read(&pixels);
        let left = pixel(read, width, width * 3 / 8, height / 2);
        let right = pixel(read, width, width * 5 / 8, height / 2);
        let corner = pixel(read, width, 2, 2);
        let said = match of {
            Of::Shown => format!("frame shown width={width} left={left} right={right} corner={corner}"),
            Of::Kept => format!("frame kept width={width} left={left} right={right} corner={corner}"),
            Of::Map | Of::Released => format!("map width={width} height={height} left={left} right={right}"),
        };
        say(said);

        Ok(())
    }
}

/// Asks the claimer to make a main frame of `window`, which another made.
#[derive(Debug, Clone, Copy, Plain)]
#[kind(name = "aether.example.claim")]
struct Claim {
    window: WindowId,
}

/// The claimer: told to, it asks to make a main frame of a window whose main frame the mapper owns, and says what it
/// was answered.
#[derive(Bound)]
#[kind(name = "aether.example.claimer")]
struct Claimer;

impl ChildOf for Claimer {
    type Parent = Top;
}

/// Who told the claimer to claim: the owner it expects to be named.
#[derive(Plain)]
#[kind(name = "aether.example.told")]
struct Told {
    by: u64,
}

impl Actor for Claimer {
    type Config = Nothing;
    type Key = ();
    type Handles = (Claim, Answer<Framed, Told>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Nothing, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Claim> for Claimer {
    type Sends = (Asks<Path<Renderer>, CreateFrame, Told>,);

    fn handle(&mut self, claim: Claim, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let by = turn.sender().id().bits();
        let create = match main_frame(claim.window) {
            Ok(create) => create,
            Err(why) => {
                say(format!("the claim was not built: {why:?}"));
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&create, &Told { by })?;

        Ok(())
    }
}

impl Handles<Answer<Framed, Told>> for Claimer {
    type Sends = ();

    fn handle(&mut self, (answered, told): (Answered<Framed>, Told), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let said = match answered {
            Answered::Yes(Framed::Refused(FrameRefused::Owned { owner })) => {
                let named = owner.bits() == told.by;
                format!("refused claim owned_by_mapper={named}")
            }
            answered => format!("claim answered={answered:?}"),
        };
        say(said);

        Ok(())
    }
}

/// The one pass of the glow's frame.
#[derive(Plain)]
#[kind(name = "aether.example.lighting")]
struct Lighting;

impl Pass for Lighting {}

/// The one pass of the frame that darkens the glow.
#[derive(Plain)]
#[kind(name = "aether.example.darkening")]
struct Darkening;

impl Pass for Darkening {}

/// An effect that reads nothing and writes a glow of 2.5, past what a byte holds.
const GLOW: &str = "
@fragment
fn glow(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return vec4(2.5, 2.5, 2.5, 1.0);
}
";

/// An effect that samples the glow smoothly and darkens it by a quarter, to 0.625, which a byte holds.
const DARKEN: &str = "
@group(1) @binding(0) var glow: texture_2d<f32>;
@group(1) @binding(1) var smoothly: sampler;

@fragment
fn darken(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return textureSample(glow, smoothly, uv) / 4.0;
}
";

/// The size of the glow's frame and of the frame that darkens it: a side of 8 pixels.
const GLOW_SIZE: Size = Size { width: 8, height: 8 };

/// What a byte of 0.625 reads, 0.625 of 255 rounded.
const DARKENED: u8 = 159;

/// The glower: it makes a frame that ends in floats, holding a glow past one, and a frame whose effect samples that
/// texture and darkens it into bytes. Once a few frames are drawn it reads both back.
#[derive(Bound)]
#[kind(name = "aether.example.glower")]
enum Glower {
    /// Its asks are under way.
    Making,
    /// Both frames are made: `drawn` frames have passed since.
    Drawing { glow: TextureId, darkened: TextureId, drawn: u32 },
    /// It has read back both.
    Done,
}

impl ChildOf for Glower {
    type Parent = Top;
}

/// Which effect, and then which frame, was asked for, with what was made before it.
#[derive(Plain)]
#[kind(name = "aether.example.glowing")]
enum Glowing {
    /// The glow's.
    Glow,
    /// The darkening's, once the glow's frame ended on `glow`.
    Darken { glow: TextureId },
}

/// Which texture was asked to be read back.
#[derive(Plain)]
#[kind(name = "aether.example.glowed")]
enum Glowed {
    /// The glow, of floats.
    Glow,
    /// The glow darkened, of bytes.
    Darkened,
}

impl Actor for Glower {
    type Config = Nothing;
    type Key = ();
    type Handles = (On<Start>, On<Tick>, Answer<Effected, Glowing>, Answer<Framed, Glowing>, Answer<Pixels, Glowed>);
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

/// An effect of the WGSL the blob `wgsl` holds, its fragment stage `fragment`, reading no target, binding `bindings`,
/// and drawing into `colour`.
fn effect(wgsl: BlobRef, fragment: &str, bindings: Vec<Binding>, colour: ColourFormat) -> CreateEffect {
    CreateEffect {
        wgsl,
        fragment: String::from(fragment),
        reads: Vec::new(),
        bindings,
        blend: Blend::Opaque,
        shape: Shape::flat(colour),
    }
}

impl Handles<On<Start>> for Glower {
    type Sends = (Asks<Path<Renderer>, CreateEffect, Glowing>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let wgsl = match turn.put(Vec::from(GLOW)) {
            Ok(wgsl) => wgsl,
            Err(why) => {
                say(format!("no glow asked: {why}"));
                return Ok(());
            }
        };
        let glow = effect(wgsl, "glow", Vec::new(), ColourFormat::Rgba16Float);
        turn.to(path::<Renderer>(())).ask(&glow, &Glowing::Glow)?;

        Ok(())
    }
}

impl Handles<Answer<Effected, Glowing>> for Glower {
    type Sends = (Asks<Path<Renderer>, CreateFrame, Glowing>,);

    fn handle(
        &mut self,
        (answered, glowing): (Answered<Effected>, Glowing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Effected::Made(made)) = answered else {
            say(format!("an effect was not made: {answered:?}"));
            return Ok(());
        };
        let Some(made) = EffectId::<()>::of(&made) else {
            say(format!("an effect reads what it was not asked to: {:?}", made.reads()));
            return Ok(());
        };
        let built = match glowing {
            Glowing::Glow => {
                let ends = Ends::Texture { size: GLOW_SIZE, format: TextureFormat::Rgba16Float };
                FrameBuilder::on(ends).finish(Lighting, Effect::new(&made, ()))
            }
            Glowing::Darken { glow } => {
                let smoothly = Sampling { filter: Filter::Linear, wrap: Wrap::Clamp };
                let bindings = vec![
                    Bind { group: 1, binding: 0, to: Resource::Texture(glow) },
                    Bind { group: 1, binding: 1, to: Resource::Sampler(smoothly) },
                ];
                let ends = Ends::Texture { size: GLOW_SIZE, format: TextureFormat::Rgba8 };
                FrameBuilder::on(ends).finish(Darkening, Effect::new(&made, ()).bound(bindings))
            }
        };
        let create = match built {
            Ok(create) => create,
            Err(why) => {
                say(format!("a glow frame was not built: {why:?}"));
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&create, &glowing)?;

        Ok(())
    }
}

impl Handles<Answer<Framed, Glowing>> for Glower {
    type Sends = (Asks<Path<Renderer>, CreateEffect, Glowing>,);

    fn handle(
        &mut self,
        (answered, glowing): (Answered<Framed>, Glowing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Framed::Texture { texture, .. }) = answered else {
            say(format!("a glow frame was not made: {answered:?}"));
            return Ok(());
        };
        let glow = match glowing {
            Glowing::Glow => texture,
            Glowing::Darken { glow } => {
                say(format!("made darkened {texture:?}"));
                *self = Self::Drawing { glow, darkened: texture, drawn: 0 };
                return Ok(());
            }
        };
        say(format!("made glow {glow:?}"));
        let wgsl = match turn.put(Vec::from(DARKEN)) {
            Ok(wgsl) => wgsl,
            Err(why) => {
                say(format!("no darkening asked: {why}"));
                return Ok(());
            }
        };
        let bindings = vec![
            Binding { group: 1, binding: 0, kind: BindingKind::Texture },
            Binding { group: 1, binding: 1, kind: BindingKind::Sampler },
        ];
        let darken = effect(wgsl, "darken", bindings, ColourFormat::Rgba8);
        turn.to(path::<Renderer>(())).ask(&darken, &Glowing::Darken { glow })?;

        Ok(())
    }
}

impl Handles<On<Tick>> for Glower {
    type Sends = (Asks<Path<Renderer>, ReadBack, Glowed>,);

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        match *self {
            Self::Drawing { glow, darkened, drawn } if drawn < SETTLED => {
                *self = Self::Drawing { glow, darkened, drawn: drawn + 1 };
            }
            Self::Drawing { glow, darkened, .. } => {
                turn.to(path::<Renderer>(()))
                    .ask(&ReadBack { of: Readable::Texture(glow) }, &Glowed::Glow)?
                    .ask(&ReadBack { of: Readable::Texture(darkened) }, &Glowed::Darkened)?;
                *self = Self::Done;
            }
            Self::Making | Self::Done => {}
        }

        Ok(())
    }
}

/// The finite float the first two bytes of `pixels` hold, a 16-bit float, little-endian: the red of a pixel of
/// `Rgba16Float`.
fn half(pixels: &[u8]) -> f32 {
    let bits = pixels.first_chunk::<2>().map_or(0, |bytes| u16::from_le_bytes(*bytes));
    let exponent = i32::from((bits >> 10) & 0x1f);
    let fraction = f32::from(bits & 0x3ff) / 1024.0;
    let magnitude = if exponent == 0 {
        fraction * 2.0_f32.powi(-14)
    } else {
        (1.0 + fraction) * 2.0_f32.powi(exponent - 15)
    };
    let negative = bits >> 15 == 1;

    if negative {
        -magnitude
    } else {
        magnitude
    }
}

impl Handles<Answer<Pixels, Glowed>> for Glower {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, glowed): (Answered<Pixels>, Glowed),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Pixels::Read { format, pixels, .. }) = answered else {
            say(format!("unread glow={answered:?}"));
            return Ok(());
        };
        let read = turn.read(&pixels);
        let said = match glowed {
            Glowed::Glow => format!("glow format={format:?} bytes={} red={}", read.len(), half(read)),
            Glowed::Darkened => {
                let byte = read.first().copied().unwrap_or_default();
                let near = byte.abs_diff(DARKENED) <= 1;
                format!("darkened format={format:?} red={byte} expected={DARKENED} near={near}")
            }
        };
        say(said);

        Ok(())
    }
}
