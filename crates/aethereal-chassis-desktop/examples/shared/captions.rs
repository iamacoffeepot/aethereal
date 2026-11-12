//! Lines of text over the window example's ring, for eyes to judge the renderer's text by (ADR-0052): its own
//! proportional font at 13, 18, and 32 pixels, its monospace font at 13, a line of two coloured runs, an outlined line
//! over the ring, and a line clipped by a box. Each line is asked for, then placed over the window's main pass once it
//! is made, and listed, top to bottom, in the captions' draw list, which the program's root list calls after the ring
//! (ADR-0055).

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, Level, On, Path, Start, Tells, Top,
    Turn, path,
};
use aethereal_data::{Bound, Plain};
use aethereal_math::{Rect2, Vec2};
use aethereal_render::Renderer;
use aethereal_render::draw_list::{DrawList, Entry, Pointer};
use aethereal_render::frame::Frame;
use aethereal_render::pass::Main;
use aethereal_render::placement::PlacementId;
use aethereal_render::text::{
    Align, Clip, Colour, CreateText, Font, Layer, Outline, PlaceText, Run, TextJoin, TextMade, TextPlaced, Wrap,
    widest_outline,
};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_window::{Cursor, InitialWindows, WindowId};

/// A grey that reads on the ring's black.
const PALE: Colour = Colour::opaque(0.8, 0.8, 0.8);

/// The captions draw on the first window the window root opened.
impl<H: Host> Bootstrap<H> for Captions {
    const SECTION: &'static str = "captions";
    type Settings = ();
    type Needs = (InitialWindows,);
    type Provides = ();

    async fn bootstrap(
        (): (),
        (windows,): (InitialWindows,),
        cx: &mut Bootstrapping<'_, H, Self>,
    ) -> Result<(), Unbuilt> {
        cx.begin(&(), Captioned { window: windows.first() })
    }
}

/// The captions' config: the window they draw on.
#[derive(Plain)]
#[kind(name = "aether.example.captioned")]
pub struct Captioned {
    window: WindowId,
}

/// The captions: they ask for each line's text as the program starts, place each as it is made, and list each placed,
/// top to bottom.
#[derive(Bound)]
#[kind(name = "aether.example.captions")]
pub struct Captions {
    window: WindowId,
    /// Each line placed so far, by its number, beside its placement, in the order of the numbers.
    placed: Vec<(u32, PlacementId)>,
}

/// Which line a text or its placement was asked for, counted from zero.
#[derive(Plain, Debug, Clone, Copy)]
#[kind(name = "aether.example.caption")]
pub struct Caption {
    line: u32,
}

/// One line: its text, made of runs, where its top left is placed, and its clip.
struct Line {
    text: CreateText,
    at: Vec2,
    clip: Clip,
}

impl ChildOf for Captions {
    type Parent = Top;
}

impl Actor for Captions {
    type Config = Captioned;
    type Key = ();
    type Handles = (On<Start>, Answer<TextMade, Caption>, Answer<TextPlaced, Caption>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(captioned: Captioned, turn: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();

        Ok(Self { window: captioned.window, placed: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A line of `runs` in `font` at `size`, outlined as `outline`, its top left at `x`, `y`, clipped as `clip` says.
fn line(font: Font, size: f32, runs: Vec<Run>, outline: Outline, (x, y): (f32, f32), clip: Clip) -> Line {
    let text = CreateText { font, size, runs, wrap: Wrap::None, align: Align::Left, outline };

    Line { text, at: Vec2::new(x, y), clip }
}

/// The lines, top to bottom.
fn lines() -> Vec<Line> {
    let pale = |text| vec![Run::new(text, PALE)];
    let sphinx = "Sphinx of black quartz, judge my vow. 0123456789";
    let runs = vec![
        Run::new("Two runs: ", Colour::opaque(0.35, 0.65, 1.0)),
        Run::new("this part is another colour", Colour::opaque(1.0, 0.45, 0.1)),
    ];
    // The widest rim the size allows, in a colour that shows over the ring's black.
    let outline = Outline::Of { width: widest_outline(32.0), colour: Colour::opaque(0.9, 0.45, 0.05) };
    let clip = Clip::To(Rect2 { min: Vec2::new(24.0, 300.0), max: Vec2::new(260.0, 330.0) });

    vec![
        line(Font::Sans, 13.0, pale(&format!("Sans 13 px. {sphinx}")), Outline::None, (24.0, 24.0), Clip::None),
        line(Font::Sans, 18.0, pale(&format!("Sans 18 px. {sphinx}")), Outline::None, (24.0, 48.0), Clip::None),
        line(Font::Sans, 32.0, pale("Sans 32 px. The quick brown fox"), Outline::None, (24.0, 80.0), Clip::None),
        line(
            Font::Mono,
            13.0,
            pale("Mono 13 px. fn main() { let x = 0x2A; } // 0O 1lI"),
            Outline::None,
            (24.0, 132.0),
            Clip::None,
        ),
        line(Font::Sans, 18.0, runs, Outline::None, (24.0, 160.0), Clip::None),
        line(
            Font::Sans,
            32.0,
            vec![Run::new("Outlined, over the ring", Colour::WHITE)],
            outline,
            (380.0, 470.0),
            Clip::None,
        ),
        line(
            Font::Sans,
            18.0,
            pale("Clipped by a box: this line runs on past the edge of its box"),
            Outline::None,
            (24.0, 304.0),
            clip,
        ),
    ]
}

impl Handles<On<Start>> for Captions {
    type Sends = (Asks<Path<Renderer>, CreateText, Caption>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        for (line, at) in lines().into_iter().zip(0..) {
            turn.to(path::<Renderer>(())).ask(&line.text, &Caption { line: at })?;
        }

        Ok(())
    }
}

impl Handles<Answer<TextMade, Caption>> for Captions {
    type Sends = (Asks<Path<Renderer>, PlaceText, Caption>,);

    fn handle(
        &mut self,
        (answered, caption): (Answered<TextMade>, Caption),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(TextMade::Made(text)) = answered else {
            turn.log(Level::Error, format_args!("{caption:?} was not made: {answered:?}"));
            return Ok(());
        };
        let Some(line) = lines().into_iter().nth(caption.line as usize) else {
            return Ok(());
        };
        let place = PlaceText {
            text,
            at: line.at,
            layer: Layer::Over,
            opacity: 1.0,
            clip: line.clip,
            joins: vec![TextJoin::to::<Main>(Frame::Main(self.window))],
        };
        turn.to(path::<Renderer>(())).ask(&place, &caption)?;

        Ok(())
    }
}

/// A line placed is listed in its place among the others, top to bottom.
impl Handles<Answer<TextPlaced, Caption>> for Captions {
    type Sends = (Tells<Path<Renderer>, DrawList>,);

    fn handle(
        &mut self,
        (answered, caption): (Answered<TextPlaced>, Caption),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(TextPlaced::Made(placement)) = answered else {
            turn.log(Level::Error, format_args!("{caption:?} was not placed: {answered:?}"));
            return Ok(());
        };
        let at = self.placed.partition_point(|(line, _)| *line < caption.line);
        self.placed.insert(at, (caption.line, placement));
        let entries = self.placed.iter().map(|(_, placement)| Entry::Draw {
            placement: *placement,
            pointer: Pointer::Ignores,
            cursor: Cursor::Default,
        });
        turn.to(path::<Renderer>(())).send(&DrawList { entries: entries.collect() })?;

        Ok(())
    }
}
