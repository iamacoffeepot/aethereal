//! The page's entry points, an actor that draws a map into a texture frame and shows it in the window's main frame it
//! makes, and an actor that asks to replace that main frame.

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, BlobRef, ChildOf, Ending, Fault, Handles, On, Path, Render, Start, Tells,
    Tick, Top, Turn, path,
};
use aethereal_chassis_browser::{Browser, Performance, open, renderer};
use aethereal_data::{Bound, Kind, KindName, Plain};
use aethereal_render::frame::builder::{FrameBuilder, Onto, depth};
use aethereal_render::frame::{CreateFrame, Ends, Frame, FrameId, FrameRefused, Framed, ShapeOf, Shaped, Size};
use aethereal_render::mesh::{Attribute, CreateMesh, MeshId, Meshed, VertexFormat, VertexLayout};
use aethereal_render::pass::{Main, Pass};
use aethereal_render::placement::{Bind, Drawn, Filter, Join, Place, Placed, Resource, Sampling, Wrap};
use aethereal_render::program::{
    Binding, BindingKind, Blend, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId, Programmed,
};
use aethereal_render::readback::{Pixels, ReadBack, Readable};
use aethereal_render::shape::{ColourFormat, Shape};
use aethereal_render::texture::TextureId;
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
/// the GPU, the claimer, and the mapper as its roots. Its blobs hold a frame of a large canvas.
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
    let (gpu, shown) = open(canvas, &mut opening).await?;
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
struct Nothing;

impl Kind for Nothing {
    const NAME: KindName = KindName::new("aether.example.nothing");
}

/// The mapper's config: the window it draws on.
#[derive(Plain)]
struct Shows {
    window: WindowId,
}

impl Kind for Shows {
    const NAME: KindName = KindName::new("aether.example.shows");
}

/// The one pass of the map's frame.
#[derive(Plain)]
struct Mapping;

impl Kind for Mapping {
    const NAME: KindName = KindName::new("aether.example.mapping");
}

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

impl Kind for Mapper {
    const NAME: KindName = KindName::new("aether.example.mapper");
}

impl ChildOf for Mapper {
    type Parent = Top;
}

/// What was learned before the quad was asked for: the window's colour, which the quad is drawn in.
#[derive(Plain)]
struct Learned {
    window: ColourFormat,
}

impl Kind for Learned {
    const NAME: KindName = KindName::new("aether.example.learned");
}

/// Which program was asked for, with what was made before it.
#[derive(Plain)]
enum Making {
    /// The map's, once the quad was made.
    Map { window: ColourFormat, mesh: MeshId },
    /// The quad's, once the map's was.
    Show { mesh: MeshId, map: ProgramId },
}

impl Kind for Making {
    const NAME: KindName = KindName::new("aether.example.making");
}

/// Which frame was asked for, with what was made before it.
#[derive(Plain)]
enum Framing {
    /// The map's, once both programs were made.
    Map { mesh: MeshId, map: ProgramId, show: ProgramId },
    /// The window's, once the map was placed in its frame.
    Main { mesh: MeshId, show: ProgramId, map: FrameId, texture: TextureId },
}

impl Kind for Framing {
    const NAME: KindName = KindName::new("aether.example.framing");
}

/// Which placement was asked for, with what was made before it.
#[derive(Plain)]
enum Placing {
    /// The map's, in its frame.
    Map { mesh: MeshId, show: ProgramId, map: FrameId, texture: TextureId },
    /// The quad that shows it, in the window's.
    Show { map: FrameId, texture: TextureId },
}

impl Kind for Placing {
    const NAME: KindName = KindName::new("aether.example.placing");
}

/// What was asked to be read back.
#[derive(Plain)]
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

impl Kind for Of {
    const NAME: KindName = KindName::new("aether.example.of");
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
        let create = match FrameBuilder::on(Ends::Texture(MAP_SIZE)).finish(Mapping, Onto(depth::Off)) {
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
struct Claim {
    window: WindowId,
}

impl Kind for Claim {
    const NAME: KindName = KindName::new("aether.example.claim");
}

/// The claimer: told to, it asks to make a main frame of a window whose main frame the mapper owns, and says what it
/// was answered.
#[derive(Bound)]
struct Claimer;

impl Kind for Claimer {
    const NAME: KindName = KindName::new("aether.example.claimer");
}

impl ChildOf for Claimer {
    type Parent = Top;
}

/// Who told the claimer to claim: the owner it expects to be named.
#[derive(Plain)]
struct Told {
    by: u64,
}

impl Kind for Told {
    const NAME: KindName = KindName::new("aether.example.told");
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
