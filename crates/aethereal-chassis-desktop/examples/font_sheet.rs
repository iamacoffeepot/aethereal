//! A study sheet of candidate default fonts, drawn by the renderer's own text (ADR-0052), in an engine linked to a hub
//! so its `capture_frame` tool can show the sheet. `--sheet sans` or `--sheet mono` picks five candidate faces, read
//! from TrueType files on this machine as the program starts and made with `CreateFont`; each is drawn in a block of
//! its own, on a slightly lighter panel over a dark grey, with the lines a game's overlay would show in it, at fixed
//! pixel sizes whatever the window's size. Each block is labelled in the renderer's own `Font::Sans` at 13 px.
//!
//! Run it as `linked_window` is run, with the arguments a hub's `mint_token` answers, plus `--sheet` and `--size`.
//! A study spike: the font paths are this study machine's.

#![allow(clippy::print_stdout, clippy::print_stderr)] // An example prints what it heard.

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, Level, On, Path, Render, Start, Tick,
    Top, Turn, path,
};
use aethereal_chassis_desktop::{Desktop, WindowsSettings};
use aethereal_chassis_headless::Exit;
use aethereal_chassis_headless::native::{Launched, NativeLayers};
use aethereal_data::{Bound, Plain};
use aethereal_files::Files;
use aethereal_link::{Link, LinkSettings};
use aethereal_math::Vec2;
use aethereal_pace::{PaceTool, Pacer};
use aethereal_render::frame::{Frame, ShapeOf, Shaped};
use aethereal_render::mesh::{Attribute, CreateMesh, MeshId, Meshed, VertexFormat, VertexLayout};
use aethereal_render::pass::Main;
use aethereal_render::placement::{Bind, Drawn, Join, Place, Placed, Resource};
use aethereal_render::program::{
    Binding, BindingKind, Blend, Coverage, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId, Programmed,
};
use aethereal_render::shape::Shape;
use aethereal_render::text::{
    Align, Clip, Colour, CreateFont, CreateText, Font, FontId, FontMade, Layer, Outline, PlaceText, Run, TextJoin,
    TextMade, TextPlaced, Wrap,
};
use aethereal_render::uniforms::{CreateUniforms, Uniformed};
use aethereal_render::{CaptureTool, RecordTool, Renderer, TargetTool};
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Unbuilt};
use aethereal_tcp::Tcp;
use aethereal_tools::{LogsTool, Toolbox};
use aethereal_window::{InitialWindows, InputTool, WindowId, WindowTool, Windows};
use core::error::Error;
use core::num::{NonZeroU32, NonZeroUsize};
use std::fs;
use std::process::ExitCode;
use std::thread;

/// The most workers the program runs on.
const MOST_WORKERS: NonZeroUsize = NonZeroUsize::new(4).expect("4 is not zero");

/// The most bytes of a frame on the link: room for an image of a large frame.
const MOST_LINK_BYTES: u32 = 16 << 20;

/// The sans candidates, in row order: each one's label and TrueType file.
const SANS: [(&str, &str); 5] = [
    ("Inter", "/mnt/cache/tmp/fonts-dl/Inter-Regular.ttf"),
    ("IBM Plex Sans", "/mnt/cache/tmp/fonts-dl/IBMPlexSans-Regular.ttf"),
    ("Fira Sans", "/mnt/cache/tmp/fonts-dl/FiraSans-Regular.ttf"),
    ("Noto Sans", "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf"),
    ("Liberation Sans", "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf"),
];

/// The mono candidates, in row order: each one's label and TrueType file.
const MONO: [(&str, &str); 5] = [
    ("JetBrains Mono", "/mnt/cache/tmp/fonts-dl/JetBrainsMono-Regular.ttf"),
    ("IBM Plex Mono", "/mnt/cache/tmp/fonts-dl/IBMPlexMono-Regular.ttf"),
    ("Fira Mono", "/mnt/cache/tmp/fonts-dl/FiraMono-Regular.ttf"),
    ("Noto Sans Mono", "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf"),
    ("Liberation Mono", "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf"),
];

/// The most boxes the backdrop program shades.
const MOST_BOXES: usize = 16;

/// A program that covers the frame and shades each pixel the colour of the last box it falls in, or the background:
/// boxes in the frame's pixels, from its top left.
const BACKDROP: &str = "
struct Boxes {
    corners: array<vec4<f32>, 16>,
    colours: array<vec4<f32>, 16>,
    background: vec4<f32>,
}

@group(0) @binding(0) var<uniform> boxes: Boxes;

@vertex
fn place(@location(0) corner: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4(corner, 0.0, 1.0);
}

@fragment
fn shade(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    var colour = boxes.background;
    for (var i = 0u; i < 16u; i++) {
        let corners = boxes.corners[i];
        let inside = at.x >= corners.x && at.y >= corners.y && at.x < corners.z && at.y < corners.w;
        if inside {
            colour = boxes.colours[i];
        }
    }
    return colour;
}
";

fn main() -> ExitCode {
    match run() {
        Ok(Launched::Ran(exit)) => {
            println!("exit={exit:?}");
            ExitCode::SUCCESS
        }
        Ok(Launched::Printed) => ExitCode::SUCCESS,
        Err(failed) => {
            eprintln!("{failed}");
            ExitCode::FAILURE
        }
    }
}

/// Launches the sheet with the link to the hub beside it, and a toolbox holding `capture_frame` for the link.
fn run() -> Result<Launched<Exit>, Box<dyn Error>> {
    let workers = NonZeroU32::try_from(
        thread::available_parallelism().map_or(NonZeroUsize::MIN, |cores| cores.min(MOST_WORKERS)),
    )?;
    let link = LinkSettings { most_bytes: MOST_LINK_BYTES, ..LinkSettings::default() };
    let engine = EngineSettings {
        workers,
        parts: workers,
        read_limit: MOST_LINK_BYTES,
        mail_bytes: 1 << 24,
        ..Default::default()
    };

    Ok(Desktop::program()
        .with_cycle::<(Tick, Render)>()
        .with_engine(engine)
        .root::<Windows>()
        .root::<Renderer>()
        .root::<Pacer>()
        .root::<FontSheet>()
        .root::<Files>()
        .root::<Tcp>()
        .root_with::<Link>(link)
        .root::<Toolbox<(CaptureTool, RecordTool, TargetTool, LogsTool, InputTool, WindowTool, PaceTool), Link>>()
        .flags(|flags| {
            flags.flag("size", 's', WindowsSettings::FIRST_SIZE).flag("mode", 'm', WindowsSettings::FIRST_MODE)
        })
        .launch("font_sheet")?)
}

/// Which candidates the sheet shows.
#[derive(Plain, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Sheet {
    /// The five proportional faces.
    #[default]
    Sans,
    /// The five monospace faces.
    Mono,
}

impl Sheet {
    /// The candidates, in row order.
    const fn candidates(self) -> [(&'static str, &'static str); 5] {
        match self {
            Self::Sans => SANS,
            Self::Mono => MONO,
        }
    }
}

/// What the sheet shows: its section, `font_sheet`.
#[derive(Plain, Default)]
#[kind(name = "aether.example.font_sheet_settings")]
#[settings]
pub struct SheetSettings {
    /// Which candidates the sheet shows: `sans` or `mono`.
    #[flag]
    sheet: Sheet,
}

/// The sheet draws on the first window the window root opened.
impl<H: Host> Bootstrap<H> for FontSheet {
    const SECTION: &'static str = "font_sheet";
    type Settings = SheetSettings;
    type Needs = (InitialWindows,);
    type Provides = ();

    async fn bootstrap(
        settings: SheetSettings,
        (windows,): (InitialWindows,),
        cx: &mut Bootstrapping<'_, H, Self>,
    ) -> Result<(), Unbuilt> {
        cx.begin(&(), Sheeted { window: windows.first(), sheet: settings.sheet })
    }
}

/// The sheet's config: the window it draws on, and which candidates.
#[derive(Plain)]
#[kind(name = "aether.example.sheeted")]
pub struct Sheeted {
    window: WindowId,
    sheet: Sheet,
}

/// One candidate's font, as the renderer answered it.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Candidate {
    /// Its font is asked for, and not yet answered.
    Asked,
    /// Its font is made.
    Made(FontId),
    /// Its file was not read, or its font was not made: its block is drawn in the built-in Sans, labelled so.
    Missing,
}

/// The sheet: it asks for its candidates' fonts and its backdrop as the program starts, then each line's text once
/// every font is answered, and places each text as it is made.
#[derive(Bound)]
#[kind(name = "aether.example.font_sheet")]
pub struct FontSheet {
    window: WindowId,
    sheet: Sheet,
    candidates: Vec<Candidate>,
}

impl ChildOf for FontSheet {
    type Parent = Top;
}

/// Which candidate a font was asked for, counted from zero.
#[derive(Plain, Debug, Clone, Copy)]
#[kind(name = "aether.example.font_asked")]
pub struct FontAsked {
    candidate: u32,
}

/// Which line a text or its placement was asked for, counted from zero.
#[derive(Plain, Debug, Clone, Copy)]
#[kind(name = "aether.example.sheet_line")]
pub struct SheetLine {
    line: u32,
}

/// The backdrop's shape, learned before its mesh was asked for.
#[derive(Plain)]
#[kind(name = "aether.example.backdrop_shaping")]
pub struct BackdropShaping;

/// What was learned before the backdrop's mesh was asked for.
#[derive(Plain)]
#[kind(name = "aether.example.backdrop_learned")]
pub struct BackdropLearned {
    shape: Shape,
}

/// What was made before the backdrop's program was asked for.
#[derive(Plain)]
#[kind(name = "aether.example.backdrop_meshed")]
pub struct BackdropMeshed {
    mesh: MeshId,
}

/// What was made before the backdrop's boxes were asked for.
#[derive(Plain)]
#[kind(name = "aether.example.backdrop_programmed")]
pub struct BackdropProgrammed {
    mesh: MeshId,
    program: ProgramId,
}

/// The backdrop's placement, asked for.
#[derive(Plain)]
#[kind(name = "aether.example.backdrop_placing")]
pub struct BackdropPlacing;

impl Actor for FontSheet {
    type Config = Sheeted;
    type Key = ();
    type Handles = (
        On<Start>,
        Answer<FontMade, FontAsked>,
        Answer<TextMade, SheetLine>,
        Answer<TextPlaced, SheetLine>,
        Answer<Shaped, BackdropShaping>,
        Answer<Meshed, BackdropLearned>,
        Answer<Programmed, BackdropMeshed>,
        Answer<Uniformed, BackdropProgrammed>,
        Answer<Placed, BackdropPlacing>,
    );
    type BeginSends = ();
    type EndSends = ();

    fn begin(sheeted: Sheeted, turn: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();

        Ok(Self { window: sheeted.window, sheet: sheeted.sheet, candidates: vec![Candidate::Asked; 5] })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// The linear-light colour of the sRGB hex colour `hex`, opaque.
fn hex(hex: u32) -> Colour {
    let channel = |shift: u32| {
        // A byte, which an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let encoded = ((hex >> shift) & 0xff) as f32 / 255.0;
        if encoded <= 0.040_45 {
            encoded / 12.92
        } else {
            ((encoded + 0.055) / 1.055).powf(2.4)
        }
    };

    Colour::opaque(channel(16), channel(8), channel(0))
}

/// The window's background.
const BACKGROUND: u32 = 0x001b_1d22;
/// A candidate's panel.
const PANEL: u32 = 0x0026_2a31;
/// The bright stripe a nameplate sits over.
const STRIPE: u32 = 0x00d9_c38a;
/// The block labels.
const LABEL: u32 = 0x007d_838d;
/// The body text.
const BODY: u32 = 0x00dd_e1e8;
/// The log timestamps.
const STAMP: u32 = 0x0086_8c96;
/// The INFO level word.
const INFO: u32 = 0x008c_cfff;
/// The WARN level word.
const WARN: u32 = 0x00ff_b347;

/// Where each block sits: its top left and size, in the frame's pixels.
#[derive(Clone, Copy)]
struct Block {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

/// A box's corners, left, top, right, and bottom, in the frame's pixels, and its colour.
type Shaded = ((f32, f32, f32, f32), Colour);

/// The margin round the sheet and between blocks.
const GAP: f32 = 16.0;
/// The padding inside a block.
const PAD: f32 = 14.0;

impl Sheet {
    /// The five blocks: the mono sheet in one column, each block two samples wide; the sans sheet three blocks a row.
    fn blocks(self) -> Vec<Block> {
        match self {
            Self::Mono => (0..5u8)
                .map(|at| Block { x: GAP, y: f32::from(at).mul_add(176.0 + GAP, GAP), width: 1280.0, height: 176.0 })
                .collect(),
            Self::Sans => (0..5u8)
                .map(|at| {
                    let (column, row) = (f32::from(at % 3), f32::from(at / 3));
                    Block {
                        x: column.mul_add(600.0 + GAP, GAP),
                        y: row.mul_add(260.0 + GAP, GAP),
                        width: 600.0,
                        height: 260.0,
                    }
                })
                .collect(),
        }
    }

    /// The nameplate stripe in `block`, on the sans sheet.
    fn stripe(block: Block) -> (f32, f32, f32, f32) {
        (block.x + PAD, block.y + 204.0, block.x + 340.0, block.y + 240.0)
    }
}

/// One line: its text, and where its top left is placed.
struct Line {
    text: CreateText,
    at: Vec2,
}

/// A text of `runs` in `font` at `size`, outlined as `outline`, its top left at `x`, `y`.
fn line(font: Font, size: f32, runs: Vec<Run>, outline: Outline, x: f32, y: f32) -> Line {
    let text = CreateText { font, size, runs, wrap: Wrap::None, align: Align::Left, outline };

    Line { text, at: Vec2::new(x, y) }
}

/// The mono sample: four log and console lines, the timestamps dim and the level words coloured.
fn mono_runs() -> Vec<Run> {
    let (body, stamp) = (hex(BODY), hex(STAMP));

    vec![
        Run::new("12:04:31.207 ", stamp),
        Run::new("INFO", hex(INFO)),
        Run::new("  ichor.city   baked 412 chunks in 38.2 ms\n", body),
        Run::new("12:04:31.950 ", stamp),
        Run::new("WARN", hex(WARN)),
        Run::new("  aether.link  engine 3 slow: p99 21.4 ms > 16.7 ms\n", body),
        Run::new("> tune aether.example.scene {\"spin\": 0.25}\n", body),
        Run::new("0O 1lI| {}[]() <= != => ~ `quoted` \"double\"", body),
    ]
}

impl FontSheet {
    /// The font each candidate's block is drawn in, and its label.
    fn faces(&self) -> Vec<(Font, String)> {
        self.sheet
            .candidates()
            .iter()
            .zip(&self.candidates)
            .zip(1..)
            .map(|(((name, _), candidate), number)| match candidate {
                Candidate::Made(font) => (Font::Made(*font), format!("{number} {name}")),
                Candidate::Asked | Candidate::Missing => {
                    (Font::Sans, format!("{number} {name} (MISSING: built-in Sans)"))
                }
            })
            .collect()
    }

    /// Every line of the sheet, block by block.
    fn lines(&self) -> Vec<Line> {
        let body = |text: &str| vec![Run::new(text, hex(BODY))];
        let nameplate = Outline::Of { width: 1.5, colour: Colour::BLACK };

        self.sheet
            .blocks()
            .into_iter()
            .zip(self.faces())
            .flat_map(|(block, (font, label))| {
                let (x, y) = (block.x + PAD, block.y + 10.0);
                let mut lines = vec![line(Font::Sans, 13.0, vec![Run::new(&label, hex(LABEL))], Outline::None, x, y)];
                match self.sheet {
                    Sheet::Mono => {
                        lines.push(line(font, 13.0, mono_runs(), Outline::None, x, y + 30.0));
                        lines.push(line(font, 14.0, mono_runs(), Outline::None, x + 620.0, y + 30.0));
                    }
                    Sheet::Sans => {
                        let (_, stripe_top, _, _) = Sheet::stripe(block);
                        let plate = vec![Run::new("Gravewarden Ysolde", Colour::WHITE)];
                        lines.extend([
                            line(
                                font,
                                14.0,
                                body("Inventory   Equip   Drop   Sell 1,240 gold"),
                                Outline::None,
                                x,
                                y + 30.0,
                            ),
                            line(
                                font,
                                14.0,
                                body("Health 412 / 560   Mana 88 / 120   Level 14"),
                                Outline::None,
                                x,
                                y + 56.0,
                            ),
                            line(font, 18.0, body("The Sunken Archive \u{2014} Floor 3"), Outline::None, x, y + 84.0),
                            line(font, 32.0, body("Ichor 1234567890"), Outline::None, x, y + 120.0),
                            line(font, 18.0, plate, nameplate, x + 12.0, stripe_top + 7.0),
                        ]);
                    }
                }
                lines
            })
            .collect()
    }

    /// The backdrop's boxes: each block's panel, and on the sans sheet each nameplate's stripe, with the background.
    fn boxes(&self) -> Vec<u8> {
        let blocks = self.sheet.blocks();
        let panels =
            blocks.iter().map(|block| ((block.x, block.y, block.x + block.width, block.y + block.height), hex(PANEL)));
        let stripes =
            blocks.iter().filter(|_| self.sheet == Sheet::Sans).map(|block| (Sheet::stripe(*block), hex(STRIPE)));
        let mut boxes: Vec<Shaded> = panels.chain(stripes).collect();
        boxes.resize(MOST_BOXES, ((0.0, 0.0, 0.0, 0.0), Colour::CLEAR));
        let corners = boxes.iter().flat_map(|((left, top, right, bottom), _)| [*left, *top, *right, *bottom]);
        let colours = boxes.iter().flat_map(|(_, colour)| colour.channels());

        corners.chain(colours).chain(hex(BACKGROUND).channels()).flat_map(f32::to_ne_bytes).collect()
    }
}

impl Handles<On<Start>> for FontSheet {
    type Sends = (Asks<Path<Renderer>, CreateFont, FontAsked>, Asks<Path<Renderer>, ShapeOf, BackdropShaping>);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.to(path::<Renderer>(())).ask(&ShapeOf::pass::<Main>(Frame::Main(self.window)), &BackdropShaping)?;
        for ((name, file), at) in self.sheet.candidates().into_iter().zip(0u32..) {
            // A file not read is asked for as no bytes, which the renderer refuses, so its block shows it missing.
            let read = fs::read(file).unwrap_or_else(|failed| {
                turn.log(Level::Error, format_args!("{name} was not read from {file}: {failed}"));
                Vec::new()
            });
            let Ok(bytes) = turn.put(read) else {
                turn.log(Level::Error, format_args!("{name}'s bytes were not put"));
                return Ok(());
            };
            turn.to(path::<Renderer>(())).ask(&CreateFont { bytes }, &FontAsked { candidate: at })?;
        }

        Ok(())
    }
}

/// What asking for the lines' texts sends.
type AsksTexts = (Asks<Path<Renderer>, CreateText, SheetLine>,);

impl FontSheet {
    /// Asks for every line's text, once every candidate's font is answered.
    fn ask_texts(&self, turn: &mut Turn<'_, Self, AsksTexts>) -> Result<(), Fault> {
        let waiting = self.candidates.contains(&Candidate::Asked);
        if waiting {
            return Ok(());
        }
        for (line, at) in self.lines().into_iter().zip(0..) {
            turn.to(path::<Renderer>(())).ask(&line.text, &SheetLine { line: at })?;
        }

        Ok(())
    }
}

impl Handles<Answer<FontMade, FontAsked>> for FontSheet {
    type Sends = AsksTexts;

    fn handle(
        &mut self,
        (answered, asked): (Answered<FontMade>, FontAsked),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let candidate = match answered {
            Answered::Yes(FontMade::Made(font)) => Candidate::Made(font),
            refused => {
                turn.log(Level::Error, format_args!("{asked:?} was not made: {refused:?}"));
                Candidate::Missing
            }
        };
        self.candidates[asked.candidate as usize] = candidate;
        self.ask_texts(turn)
    }
}

impl Handles<Answer<TextMade, SheetLine>> for FontSheet {
    type Sends = (Asks<Path<Renderer>, PlaceText, SheetLine>,);

    fn handle(
        &mut self,
        (answered, sheet_line): (Answered<TextMade>, SheetLine),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(TextMade::Made(text)) = answered else {
            turn.log(Level::Error, format_args!("{sheet_line:?} was not made: {answered:?}"));
            return Ok(());
        };
        let Some(line) = self.lines().into_iter().nth(sheet_line.line as usize) else {
            return Ok(());
        };
        let place = PlaceText {
            text,
            at: line.at,
            layer: Layer::Over,
            opacity: 1.0,
            clip: Clip::None,
            joins: vec![TextJoin::to::<Main>(Frame::Main(self.window))],
        };
        turn.to(path::<Renderer>(())).ask(&place, &sheet_line)?;

        Ok(())
    }
}

impl Handles<Answer<TextPlaced, SheetLine>> for FontSheet {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, sheet_line): (Answered<TextPlaced>, SheetLine),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let placed = matches!(answered, Answered::Yes(TextPlaced::Made(_)));
        if !placed {
            turn.log(Level::Error, format_args!("{sheet_line:?} was not placed: {answered:?}"));
        }

        Ok(())
    }
}

/// The bytes of `values`, as a blob is put.
fn words(values: impl IntoIterator<Item = u32>) -> Vec<u8> {
    values.into_iter().flat_map(u32::to_ne_bytes).collect()
}

impl Handles<Answer<Shaped, BackdropShaping>> for FontSheet {
    type Sends = (Asks<Path<Renderer>, CreateMesh, BackdropLearned>,);

    fn handle(
        &mut self,
        (answered, _): (Answered<Shaped>, BackdropShaping),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Shaped::Shape(shape)) = answered else {
            turn.log(Level::Error, format_args!("the main pass has no shape: {answered:?}"));
            return Ok(());
        };
        let corners = [-1.0_f32, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0].map(f32::to_bits);
        let (Ok(vertices), Ok(indices)) = (turn.put(words(corners)), turn.put(words([0, 1, 2, 0, 2, 3]))) else {
            turn.log(Level::Error, "the backdrop's quad was not put");
            return Ok(());
        };
        let position = Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 };
        let quad = CreateMesh { layout: VertexLayout { stride: 8, attributes: vec![position] }, vertices, indices };
        turn.to(path::<Renderer>(())).ask(&quad, &BackdropLearned { shape })?;

        Ok(())
    }
}

impl Handles<Answer<Meshed, BackdropLearned>> for FontSheet {
    type Sends = (Asks<Path<Renderer>, CreateProgram, BackdropMeshed>,);

    fn handle(
        &mut self,
        (answered, learned): (Answered<Meshed>, BackdropLearned),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Meshed::Made(mesh)) = answered else {
            turn.log(Level::Error, format_args!("the backdrop's quad was not made: {answered:?}"));
            return Ok(());
        };
        let Ok(wgsl) = turn.put(Vec::from(BACKDROP)) else {
            turn.log(Level::Error, "the backdrop's program was not put");
            return Ok(());
        };
        let position = Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 };
        let bytes = u32::try_from((MOST_BOXES * 2 + 1) * 16).unwrap_or(u32::MAX);
        let program = CreateProgram {
            wgsl,
            vertex: String::from("place"),
            fragment: String::from("shade"),
            layout: VertexLayout { stride: 8, attributes: vec![position] },
            instances: PerInstance::None,
            bindings: vec![Binding { group: 0, binding: 0, kind: BindingKind::Uniform { bytes } }],
            reads: Vec::new(),
            state: Fixed { blend: Blend::Opaque, depth: Depth::Off, cull: Cull::None, coverage: Coverage::Off },
            shape: learned.shape,
        };
        turn.to(path::<Renderer>(())).ask(&program, &BackdropMeshed { mesh })?;

        Ok(())
    }
}

impl Handles<Answer<Programmed, BackdropMeshed>> for FontSheet {
    type Sends = (Asks<Path<Renderer>, CreateUniforms, BackdropProgrammed>,);

    fn handle(
        &mut self,
        (answered, meshed): (Answered<Programmed>, BackdropMeshed),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Programmed::Made(program)) = answered else {
            turn.log(Level::Error, format_args!("the backdrop's program was not made: {answered:?}"));
            return Ok(());
        };
        let Ok(bytes) = turn.put(self.boxes()) else {
            turn.log(Level::Error, "the backdrop's boxes were not put");
            return Ok(());
        };
        turn.to(path::<Renderer>(()))
            .ask(&CreateUniforms { bytes }, &BackdropProgrammed { mesh: meshed.mesh, program })?;

        Ok(())
    }
}

impl Handles<Answer<Uniformed, BackdropProgrammed>> for FontSheet {
    type Sends = (Asks<Path<Renderer>, Place, BackdropPlacing>,);

    fn handle(
        &mut self,
        (answered, made): (Answered<Uniformed>, BackdropProgrammed),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Uniformed::Made(uniforms)) = answered else {
            turn.log(Level::Error, format_args!("the backdrop's boxes were not made: {answered:?}"));
            return Ok(());
        };
        let bindings = vec![Bind { group: 0, binding: 0, to: Resource::Uniforms(uniforms) }];
        let join = Join::to::<Main>(Frame::Main(self.window), made.program, bindings);
        let place = Place { mesh: made.mesh, instances: Drawn::Count(1), joins: vec![join] };
        turn.to(path::<Renderer>(())).ask(&place, &BackdropPlacing)?;

        Ok(())
    }
}

impl Handles<Answer<Placed, BackdropPlacing>> for FontSheet {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, _): (Answered<Placed>, BackdropPlacing),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let placed = matches!(answered, Answered::Yes(Placed::Made(_)));
        if !placed {
            turn.log(Level::Error, format_args!("the backdrop was not placed: {answered:?}"));
        }

        Ok(())
    }
}
