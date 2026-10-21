//! The page's entry points, and an actor that places two squares and a tint, then reads back what was drawn.

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, On, Path, Render, Start, Tick, Top,
    Turn, path,
};
use aethereal_chassis_browser::{Browser, Performance, open, renderer};
use aethereal_data::{Bound, Kind, KindName, Plain};
use aethereal_render::Renderer;
use aethereal_render::mesh::{Attribute, CreateMesh, MeshId, Meshed, VertexFormat, VertexLayout};
use aethereal_render::placement::{Bind, Drawn, Place, Placed, Resource};
use aethereal_render::program::{
    Binding, BindingKind, Blend, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId, Programmed,
};
use aethereal_render::readback::{Pixels, ReadBack, Readable};
use aethereal_render::storage::{CreateStorage, Stored};
use aethereal_render::texture::{CreateTexture, TextureFormat, TextureId, Textured};
use aethereal_render::timing::{TimeFrame, Timed};
use aethereal_substrate::Engine;
use core::cell::RefCell;
use core::mem;
use core::num::NonZeroUsize;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::wasm_bindgen;
use web_sys::HtmlCanvasElement;

thread_local! {
    /// What the reader was answered, for the page to show.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Notes what the reader was answered.
fn say(line: String) {
    SAID.with_borrow_mut(|said| said.push(line));
}

/// The program the page runs, for its `requestAnimationFrame` to call.
#[wasm_bindgen]
pub struct Program {
    browser: Browser<Performance>,
}

/// Opens WebGPU on `canvas`, then makes the engine with the renderer, handed the GPU, and the reader as its roots.
/// Its blobs hold a frame of a large canvas.
///
/// # Errors
///
/// When WebGPU will not open on the canvas, or a root does not begin.
#[wasm_bindgen]
pub async fn start(canvas: HtmlCanvasElement) -> Result<Program, JsError> {
    let gpu = open(canvas).await?;
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 1 << 20, 1 << 26, 0)
        .with_cycle::<(Tick, Render)>()
        .map_err(|unordered| JsError::new(&format!("the cycle is out of order: {unordered:?}")))?;
    let (rendering, mappings) = renderer(&mut setup, gpu);
    let engine = setup
        .add_root::<Renderer>(&(), rendering)
        .map_err(|refused| JsError::new(&format!("the renderer did not begin: {refused:?}")))?
        .add_root::<Reader>(&(), Nothing)
        .map_err(|refused| JsError::new(&format!("the reader did not begin: {refused:?}")))?
        .ready();

    Ok(Program { browser: Browser::new(engine, Performance, mappings) })
}

#[wasm_bindgen]
impl Program {
    /// Runs one frame.
    pub fn frame(&mut self) {
        self.browser.frame();
    }

    /// What the reader was answered since this was last asked, a line each.
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

/// A program that draws the quad at each splat it reads from storage, by its instance: its place, its size, and its
/// colour.
const SPLATS: &str = "
struct Splat { at: vec2<f32>, size: vec2<f32>, colour: vec4<f32> }

@group(0) @binding(0) var<storage, read> splats: array<Splat>;

struct Between {
    @builtin(position) at: vec4<f32>,
    @location(0) colour: vec4<f32>,
}

@vertex
fn place(@location(0) corner: vec2<f32>, @builtin(instance_index) at: u32) -> Between {
    let splat = splats[at];
    return Between(vec4(corner * splat.size + splat.at, 0.0, 1.0), splat.colour);
}

@fragment
fn shade(between: Between) -> @location(0) vec4<f32> {
    return between.colour;
}
";

/// A program that draws the quad over the left half of the canvas, halving the red of what it covers.
const TINT: &str = "
@vertex
fn cover(@location(0) corner: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4(corner.x * 0.5 - 0.5, corner.y, 0.0, 1.0);
}

@fragment
fn tint() -> @location(0) vec4<f32> {
    return vec4(0.5, 1.0, 1.0, 1.0);
}
";

/// The texture read back: two by two, each byte its own.
const TEXELS: [u8; 16] = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160];

/// The frames drawn before the reader reads one back, so both placements are drawn.
const SETTLED: u32 = 3;

/// The reader: it makes what it draws one ask after another, each answer carrying what was made so far, places it,
/// and once a few frames are drawn reads back a frame, its texture, and a frame's time.
#[derive(Bound)]
enum Reader {
    /// Its asks are under way, and nothing is placed.
    Making,
    /// Both are placed and the texture made: `drawn` frames have passed since.
    Drawing { texture: TextureId, drawn: u32 },
    /// It has asked for what it reads back.
    Asked,
}

impl Kind for Reader {
    const NAME: KindName = KindName::new("aether.example.reader");
}

impl ChildOf for Reader {
    type Parent = Top;
}

/// Which program was asked for, with what was made before it: an actor handles one kind of answer with one kind of
/// context.
#[derive(Plain)]
enum Making {
    /// The splats, once the quad was made.
    Splats { mesh: MeshId },
    /// The tint, once the splats were.
    Tint { mesh: MeshId, splats: ProgramId },
}

impl Kind for Making {
    const NAME: KindName = KindName::new("aether.example.making");
}

/// What was made before the storage was asked for.
#[derive(Plain)]
struct MadePrograms {
    mesh: MeshId,
    splats: ProgramId,
    tint: ProgramId,
}

impl Kind for MadePrograms {
    const NAME: KindName = KindName::new("aether.example.made_programs");
}

/// Which placement was asked for.
#[derive(Plain)]
enum Placing {
    /// The splats, with what the tint is then placed with.
    Splats { mesh: MeshId, tint: ProgramId },
    /// The tint.
    Tint,
}

impl Kind for Placing {
    const NAME: KindName = KindName::new("aether.example.placing");
}

/// What was asked to be read back.
#[derive(Plain)]
enum Of {
    Frame,
    Texture,
}

impl Kind for Of {
    const NAME: KindName = KindName::new("aether.example.of");
}

impl Actor for Reader {
    type Config = Nothing;
    type Key = ();
    type Handles = (
        On<Start>,
        On<Tick>,
        Answer<Meshed, Nothing>,
        Answer<Programmed, Making>,
        Answer<Stored, MadePrograms>,
        Answer<Placed, Placing>,
        Answer<Textured, Nothing>,
        Answer<Pixels, Of>,
        Answer<Timed, Nothing>,
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

/// The bytes of `values`, floats, as a blob is put.
fn floats(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    bytes(values.into_iter().map(f32::to_bits))
}

/// How the quad's corners are laid out: a place of two floats.
fn corners() -> VertexLayout {
    let corner = Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 };

    VertexLayout { stride: 8, attributes: vec![corner] }
}

/// A program of `wgsl`, drawn from `vertex` and `fragment` over the quad, binding `bindings`, blending by `blend`.
fn program(wgsl: &str, vertex: &str, fragment: &str, bindings: Vec<Binding>, blend: Blend) -> CreateProgram {
    CreateProgram {
        wgsl: String::from(wgsl),
        vertex: String::from(vertex),
        fragment: String::from(fragment),
        layout: corners(),
        instances: PerInstance::None,
        bindings,
        state: Fixed { blend, depth: Depth::Off, cull: Cull::None },
    }
}

impl Handles<On<Start>> for Reader {
    type Sends = (Asks<Path<Renderer>, CreateMesh, Nothing>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let quad = floats([-1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0]);
        let (vertices, indices) = match (turn.put(quad), turn.put(bytes([0, 1, 2, 0, 2, 3]))) {
            (Ok(vertices), Ok(indices)) => (vertices, indices),
            (Err(why), _) | (_, Err(why)) => {
                say(format!("nothing asked: {why}"));
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&CreateMesh { layout: corners(), vertices, indices }, &Nothing)?;

        Ok(())
    }
}

impl Handles<Answer<Meshed, Nothing>> for Reader {
    type Sends = (Asks<Path<Renderer>, CreateProgram, Making>,);

    fn handle(
        &mut self,
        (answered, _): (Answered<Meshed>, Nothing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Meshed::Made(mesh)) = answered else {
            say(format!("the quad was not made: {answered:?}"));
            return Ok(());
        };
        let storage = Binding { group: 0, binding: 0, kind: BindingKind::Storage { bytes: 32 } };
        let splats = program(SPLATS, "place", "shade", vec![storage], Blend::Opaque);
        turn.to(path::<Renderer>(())).ask(&splats, &Making::Splats { mesh })?;

        Ok(())
    }
}

impl Handles<Answer<Programmed, Making>> for Reader {
    type Sends = (Asks<Path<Renderer>, CreateProgram, Making>, Asks<Path<Renderer>, CreateStorage, MadePrograms>);

    fn handle(
        &mut self,
        (answered, making): (Answered<Programmed>, Making),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Programmed::Made(made)) = answered else {
            say(format!("a program was not made: {answered:?}"));
            return Ok(());
        };
        let (mesh, splats) = match making {
            Making::Splats { mesh } => {
                let tint = program(TINT, "cover", "tint", Vec::new(), Blend::Multiply);
                turn.to(path::<Renderer>(())).ask(&tint, &Making::Tint { mesh, splats: made })?;
                return Ok(());
            }
            Making::Tint { mesh, splats } => (mesh, splats),
        };
        // A red square left of the middle, under the tint, and a green one right of it: a place, a size, a colour.
        let splatted = floats([-0.5, 0.0, 0.3, 0.3, 1.0, 0.0, 0.0, 1.0, 0.5, 0.0, 0.3, 0.3, 0.0, 1.0, 0.0, 1.0]);
        let bytes = match turn.put(splatted) {
            Ok(bytes) => bytes,
            Err(why) => {
                say(format!("no storage asked: {why}"));
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&CreateStorage { bytes }, &MadePrograms { mesh, splats, tint: made })?;

        Ok(())
    }
}

impl Handles<Answer<Stored, MadePrograms>> for Reader {
    type Sends = (Asks<Path<Renderer>, Place, Placing>,);

    fn handle(
        &mut self,
        (answered, made): (Answered<Stored>, MadePrograms),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Stored::Made(storage)) = answered else {
            say(format!("the storage was not made: {answered:?}"));
            return Ok(());
        };
        let place = Place {
            program: made.splats,
            mesh: made.mesh,
            instances: Drawn::Count(2),
            bindings: vec![Bind { group: 0, binding: 0, to: Resource::Storage(storage) }],
        };
        turn.to(path::<Renderer>(())).ask(&place, &Placing::Splats { mesh: made.mesh, tint: made.tint })?;

        Ok(())
    }
}

impl Handles<Answer<Placed, Placing>> for Reader {
    type Sends = (Asks<Path<Renderer>, Place, Placing>, Asks<Path<Renderer>, CreateTexture, Nothing>);

    fn handle(
        &mut self,
        (answered, placing): (Answered<Placed>, Placing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Placed::Made(_)) = answered else {
            say(format!("a placement was not made: {answered:?}"));
            return Ok(());
        };
        if let Placing::Splats { mesh, tint } = placing {
            // Drawn after the splats, as the renderer draws the programs it made first first; nothing promises it.
            let place = Place { program: tint, mesh, instances: Drawn::Count(1), bindings: Vec::new() };
            turn.to(path::<Renderer>(())).ask(&place, &Placing::Tint)?;
            return Ok(());
        }
        let pixels = match turn.put(TEXELS.to_vec()) {
            Ok(pixels) => pixels,
            Err(why) => {
                say(format!("no texture asked: {why}"));
                return Ok(());
            }
        };
        let texture = CreateTexture { width: 2, height: 2, format: TextureFormat::Rgba8, pixels };
        turn.to(path::<Renderer>(())).ask(&texture, &Nothing)?;

        Ok(())
    }
}

impl Handles<Answer<Textured, Nothing>> for Reader {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, _): (Answered<Textured>, Nothing),
        _: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Textured::Made(texture)) = answered else {
            say(format!("the texture was not made: {answered:?}"));
            return Ok(());
        };
        say(String::from("placed splats tint texture"));
        *self = Self::Drawing { texture, drawn: 0 };

        Ok(())
    }
}

impl Handles<On<Tick>> for Reader {
    type Sends = (Asks<Path<Renderer>, ReadBack, Of>, Asks<Path<Renderer>, TimeFrame, Nothing>);

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Self::Drawing { texture, drawn } = *self else {
            return Ok(());
        };
        if drawn < SETTLED {
            *self = Self::Drawing { texture, drawn: drawn + 1 };
            return Ok(());
        }
        turn.to(path::<Renderer>(()))
            .ask(&ReadBack { of: Readable::Frame }, &Of::Frame)?
            .ask(&ReadBack { of: Readable::Texture(texture) }, &Of::Texture)?
            .ask(&TimeFrame, &Nothing)?;
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

impl Handles<Answer<Pixels, Of>> for Reader {
    type Sends = ();

    fn handle(&mut self, (answered, of): (Answered<Pixels>, Of), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let Answered::Yes(Pixels::Read { width, height, format, pixels }) = answered else {
            say(format!("nothing was read: {answered:?}"));
            return Ok(());
        };
        let read = turn.read(&pixels);
        let said = match of {
            Of::Frame => {
                let tinted = pixel(read, width, width / 4, height / 2);
                let green = pixel(read, width, width * 3 / 4, height / 2);
                let corner = pixel(read, width, 2, 2);
                let bytes = read.len();
                format!(
                    "frame width={width} height={height} format={format:?} bytes={bytes} tinted_red={tinted} \
                     green={green} corner={corner}"
                )
            }
            Of::Texture => {
                let same = read == TEXELS;
                format!("texture width={width} height={height} same={same} bytes={read:?}")
            }
        };
        say(said);

        Ok(())
    }
}

impl Handles<Answer<Timed, Nothing>> for Reader {
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
