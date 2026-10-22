//! The page's entry points, and an actor that places two squares and a tint, then reads back what was drawn and the
//! storage it drew from.

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, BlobRef, ChildOf, Ending, Fault, Handles, On, Path, Render, Start, Tick,
    Top, Turn, path,
};
use aethereal_chassis_browser::{Browser, Started};
use aethereal_data::{Bound, Plain};
use aethereal_render::Renderer;
use aethereal_render::frame::{Frame, ShapeOf, Shaped};
use aethereal_render::mesh::{Attribute, CreateMesh, MeshId, Meshed, VertexFormat, VertexLayout};
use aethereal_render::pass::Main;
use aethereal_render::placement::{Bind, Drawn, Join, Place, Placed, Resource};
use aethereal_render::program::{
    Access, Binding, BindingKind, Blend, Coverage, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId,
    Programmed,
};
use aethereal_render::readback::{Bytes, Pixels, ReadBack, ReadStorage, Readable};
use aethereal_render::shape::Shape;
use aethereal_render::storage::{CreateStorage, StorageId, Stored};
use aethereal_render::texture::{CreateTexture, TextureFormat, TextureId, Textured};
use aethereal_render::timing::{TimeFrame, Timed};
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Unbuilt};
use aethereal_window::{InitialWindows, WindowId, Windows};
use core::cell::RefCell;
use core::mem;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::wasm_bindgen;

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
    started: Started,
}

/// Bootstraps the program on the page with the window root, which opens the page's canvas as the one window,
/// the renderer, which opens WebGPU on it, and the reader.
///
/// # Errors
///
/// When the canvas or WebGPU on it will not open, the page will not be listened to, or a root does not begin.
#[wasm_bindgen(wasm_bindgen_futures = aethereal_chassis_browser::wasm_bindgen_futures)]
pub async fn start() -> Result<Program, JsError> {
    let started = Browser::program()
        .with_cycle::<(Tick, Render)>()
        // Its blobs hold a frame of a large canvas.
        .with_engine(EngineSettings { blob_bytes: 1 << 26, ..Default::default() })
        .root::<Windows>()
        .root::<Renderer>()
        .root::<Reader>()
        .run()?
        .await?;

    Ok(Program { started })
}

#[wasm_bindgen]
impl Program {
    /// Runs one frame, at the timestamp `requestAnimationFrame` handed its callback, in milliseconds.
    pub fn frame(&mut self, timestamp_millis: f64) {
        self.started.frame(timestamp_millis);
    }

    /// What the reader was answered since this was last asked, a line each.
    #[must_use]
    #[expect(clippy::unused_self, reason = "the page asks it of the program it started")]
    pub fn said(&self) -> Vec<String> {
        SAID.with_borrow_mut(mem::take)
    }
}

/// The reader draws on the one window the window root opened.
impl<H: Host> Bootstrap<H> for Reader {
    const SECTION: &'static str = "reader";
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

/// The reader's config: the window it draws on.
#[derive(Plain)]
#[kind(name = "aether.example.shows")]
struct Shows {
    window: WindowId,
}

/// What was learned before the quad was asked for: the shape the programs draw into.
#[derive(Plain)]
#[kind(name = "aether.example.learned")]
struct Learned {
    shape: Shape,
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

/// The splats the storage holds, and is read back: a red square left of the middle, under the tint, and a green one
/// right of it, each a place, a size, a colour.
const SPLATS_HELD: [f32; 16] = [-0.5, 0.0, 0.3, 0.3, 1.0, 0.0, 0.0, 1.0, 0.5, 0.0, 0.3, 0.3, 0.0, 1.0, 0.0, 1.0];

/// The texture read back: two by two, each byte its own.
const TEXELS: [u8; 16] = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160];

/// The frames drawn before the reader reads one back, so both placements are drawn.
const SETTLED: u32 = 3;

/// The reader: it makes what it draws one ask after another, each answer carrying what was made so far, places it,
/// and once a few frames are drawn reads back a frame, its texture, its storage, and a frame's time.
#[derive(Bound)]
#[kind(name = "aether.example.reader")]
enum Reader {
    /// Its asks are under way, and nothing is placed on `window`.
    Making { window: WindowId },
    /// Both are placed and the texture made: `drawn` frames have passed since.
    Drawing { texture: TextureId, storage: StorageId, drawn: u32 },
    /// It has asked for what it reads back.
    Asked,
}

impl ChildOf for Reader {
    type Parent = Top;
}

/// Which program was asked for, with what was made before it: an actor handles one kind of answer with one kind of
/// context.
#[derive(Plain)]
#[kind(name = "aether.example.making")]
enum Making {
    /// The splats, once the quad was made, drawing into `shape`.
    Splats { shape: Shape, mesh: MeshId },
    /// The tint, once the splats were.
    Tint { mesh: MeshId, splats: ProgramId },
}

/// What was made before the storage was asked for.
#[derive(Plain)]
#[kind(name = "aether.example.made_programs")]
struct MadePrograms {
    mesh: MeshId,
    splats: ProgramId,
    tint: ProgramId,
}

/// Which placement was asked for, with the storage the splats are drawn from.
#[derive(Plain)]
#[kind(name = "aether.example.placing")]
enum Placing {
    /// The splats, with what the tint is then placed with.
    Splats { mesh: MeshId, tint: ProgramId, storage: StorageId },
    /// The tint.
    Tint { storage: StorageId },
}

/// What the texture was asked for with: the storage the splats are drawn from, read back with it.
#[derive(Plain)]
#[kind(name = "aether.example.splatted")]
struct Splatted {
    storage: StorageId,
}

/// What was asked to be read back.
#[derive(Plain)]
#[kind(name = "aether.example.of")]
enum Of {
    Frame,
    Texture,
}

impl Actor for Reader {
    type Config = Shows;
    type Key = ();
    type Handles = (
        On<Start>,
        On<Tick>,
        Answer<Shaped, Nothing>,
        Answer<Meshed, Learned>,
        Answer<Programmed, Making>,
        Answer<Stored, MadePrograms>,
        Answer<Placed, Placing>,
        Answer<Textured, Splatted>,
        Answer<Pixels, Of>,
        Answer<Bytes, Nothing>,
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

/// The bytes of `values`, floats, as a blob is put.
fn floats(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    bytes(values.into_iter().map(f32::to_bits))
}

/// How the quad's corners are laid out: a place of two floats.
fn corners() -> VertexLayout {
    let corner = Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 };

    VertexLayout { stride: 8, attributes: vec![corner] }
}

/// A program of the WGSL the blob `wgsl` holds, drawn from `vertex` and `fragment` over the quad into `shape`, binding
/// `bindings`, blending by `blend`.
fn program(
    wgsl: BlobRef,
    (vertex, fragment): (&str, &str),
    shape: Shape,
    bindings: Vec<Binding>,
    blend: Blend,
) -> CreateProgram {
    CreateProgram {
        wgsl,
        vertex: String::from(vertex),
        fragment: String::from(fragment),
        layout: corners(),
        instances: PerInstance::None,
        bindings,
        reads: Vec::new(),
        state: Fixed { blend, depth: Depth::Off, cull: Cull::None, coverage: Coverage::Off },
        shape,
    }
}

impl Handles<On<Start>> for Reader {
    type Sends = (Asks<Path<Renderer>, ShapeOf, Nothing>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Self::Making { window } = *self else {
            return Ok(());
        };
        turn.to(path::<Renderer>(())).ask(&ShapeOf::pass::<Main>(Frame::Main(window)), &Nothing)?;

        Ok(())
    }
}

impl Handles<Answer<Shaped, Nothing>> for Reader {
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
        let quad = floats([-1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0]);
        let (vertices, indices) = match (turn.put(quad), turn.put(bytes([0, 1, 2, 0, 2, 3]))) {
            (Ok(vertices), Ok(indices)) => (vertices, indices),
            (Err(why), _) | (_, Err(why)) => {
                say(format!("nothing asked: {why}"));
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&CreateMesh { layout: corners(), vertices, indices }, &Learned { shape })?;

        Ok(())
    }
}

impl Handles<Answer<Meshed, Learned>> for Reader {
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
        let storage = Binding { group: 0, binding: 0, kind: BindingKind::Storage { bytes: 32, access: Access::Read } };
        let shape = learned.shape;
        let wgsl = match turn.put(Vec::from(SPLATS)) {
            Ok(wgsl) => wgsl,
            Err(why) => {
                say(format!("no splats asked: {why}"));
                return Ok(());
            }
        };
        let splats = program(wgsl, ("place", "shade"), shape, vec![storage], Blend::Opaque);
        turn.to(path::<Renderer>(())).ask(&splats, &Making::Splats { shape, mesh })?;

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
            Making::Splats { shape, mesh } => {
                let wgsl = match turn.put(Vec::from(TINT)) {
                    Ok(wgsl) => wgsl,
                    Err(why) => {
                        say(format!("no tint asked: {why}"));
                        return Ok(());
                    }
                };
                let tint = program(wgsl, ("cover", "tint"), shape, Vec::new(), Blend::Multiply);
                turn.to(path::<Renderer>(())).ask(&tint, &Making::Tint { mesh, splats: made })?;
                return Ok(());
            }
            Making::Tint { mesh, splats } => (mesh, splats),
        };
        let bytes = match turn.put(floats(SPLATS_HELD)) {
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
        let Self::Making { window } = *self else {
            return Ok(());
        };
        let bindings = vec![Bind { group: 0, binding: 0, to: Resource::Storage(storage) }];
        let place = Place {
            mesh: made.mesh,
            instances: Drawn::Count(2),
            joins: vec![Join::to::<Main>(Frame::Main(window), made.splats, bindings)],
        };
        let placing = Placing::Splats { mesh: made.mesh, tint: made.tint, storage };
        turn.to(path::<Renderer>(())).ask(&place, &placing)?;

        Ok(())
    }
}

impl Handles<Answer<Placed, Placing>> for Reader {
    type Sends = (Asks<Path<Renderer>, Place, Placing>, Asks<Path<Renderer>, CreateTexture, Splatted>);

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
        let storage = match placing {
            Placing::Splats { mesh, tint, storage } => {
                // Drawn after the splats, as the renderer draws the programs it made first first; nothing promises it.
                let join = Join::to::<Main>(Frame::Main(window), tint, Vec::new());
                let place = Place { mesh, instances: Drawn::Count(1), joins: vec![join] };
                turn.to(path::<Renderer>(())).ask(&place, &Placing::Tint { storage })?;
                return Ok(());
            }
            Placing::Tint { storage } => storage,
        };
        let pixels = match turn.put(TEXELS.to_vec()) {
            Ok(pixels) => pixels,
            Err(why) => {
                say(format!("no texture asked: {why}"));
                return Ok(());
            }
        };
        let texture = CreateTexture { width: 2, height: 2, format: TextureFormat::Rgba8, pixels };
        turn.to(path::<Renderer>(())).ask(&texture, &Splatted { storage })?;

        Ok(())
    }
}

impl Handles<Answer<Textured, Splatted>> for Reader {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, splatted): (Answered<Textured>, Splatted),
        _: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Textured::Made(texture)) = answered else {
            say(format!("the texture was not made: {answered:?}"));
            return Ok(());
        };
        say(String::from("placed splats tint texture"));
        *self = Self::Drawing { texture, storage: splatted.storage, drawn: 0 };

        Ok(())
    }
}

impl Handles<On<Tick>> for Reader {
    type Sends = (
        Asks<Path<Renderer>, ReadBack, Of>,
        Asks<Path<Renderer>, ReadStorage, Nothing>,
        Asks<Path<Renderer>, TimeFrame, Nothing>,
    );

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Self::Drawing { texture, storage, drawn } = *self else {
            return Ok(());
        };
        if drawn < SETTLED {
            *self = Self::Drawing { texture, storage, drawn: drawn + 1 };
            return Ok(());
        }
        turn.to(path::<Renderer>(()))
            .ask(&ReadBack { of: Readable::Frame }, &Of::Frame)?
            .ask(&ReadBack { of: Readable::Texture(texture) }, &Of::Texture)?
            .ask(&ReadStorage { storage }, &Nothing)?
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

impl Handles<Answer<Bytes, Nothing>> for Reader {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, _): (Answered<Bytes>, Nothing),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Bytes::Read { bytes }) = answered else {
            say(format!("the storage was not read: {answered:?}"));
            return Ok(());
        };
        let read = turn.read(&bytes);
        let same = read == floats(SPLATS_HELD);
        let bytes = read.len();
        say(format!("storage bytes={bytes} same={same}"));

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
