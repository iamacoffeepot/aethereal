//! A reply of Claude's in markdown, streamed into a window a few characters a frame and drawn as widgets (ADR-0056:
//! Markdown): every construct a reply uses, each block a `MarkdownBlock` kept by `memo` as the overlay's chat keeps
//! them. A press on a link logs its target; nothing is opened. The reply's image names a PNG, which the example decodes
//! once, as it begins, and hands the reply's blocks as the picture of that source.
//!
//! `cargo run --release -p aethereal-overlay --example markdown`. With `REPLY_SHOT` naming a file, once the reply
//! has streamed in, the window's frame is read back and written there as a PPM, which any image tool converts.

#![allow(clippy::print_stdout, clippy::print_stderr)] // An example prints how it ended.

use aethereal_actor::{
    Actor, Answer, Answered, Anyone, Beginning, ChildOf, Ending, Fault, Handles, Level, On, Prepare, Receives, Render,
    Tick, Top, Turn, path,
};
use aethereal_chassis_desktop::{Desktop, WindowSettings, WindowsSettings};
use aethereal_chassis_headless::Exit;
use aethereal_chassis_headless::native::{Launched, NativeLayers};
use aethereal_data::{Bound, Plain};
use aethereal_render::Renderer;
use aethereal_render::frame::Frame;
use aethereal_render::pass::Interface;
use aethereal_render::readback::{Pixels as Read, ReadBack, Readable};
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Unbuilt};
use aethereal_ui::markdown::{Document, Images, MarkdownBlock, Prose};
use aethereal_ui::pixels::Pixels;
use aethereal_ui::prelude::*;
use aethereal_window::{self as window, Event, InitialWindows, Mode, Window, WindowId, Windows};
use core::error::Error;
use core::num::{NonZeroU32, NonZeroUsize};
use std::process::ExitCode;
use std::thread;
use std::{env, fs};

/// The window's title.
const TITLE: &str = "Aethereal markdown example";

/// The most workers the program runs on.
const MOST_WORKERS: NonZeroUsize = NonZeroUsize::new(4).expect("4 is not zero");

/// How many characters of the reply come each frame.
const CHARACTERS_PER_FRAME: usize = 24;

/// How many frames after the reply has come its frame is read back.
const SETTLE_FRAMES: u32 = 30;

/// The picture the reply's image names, as its bytes would arrive.
const PIPELINE: &[u8] = include_bytes!("shared/pipeline.png");

/// A reply that uses every construct.
const REPLY: &str = r#"# Markdown in the overlay

A reply is read into **blocks**, each built into widgets: *emphasis*, **strong**, ~~struck~~, `inline code`, and a
[link to the docs](https://example.com/docs), all runs of one text.

## Lists

- Bullets, as many as there are
- Nested ones:
    1. numbered from one
    2. and on
- and back out

### A code block

```rust
fn main() {
    let reply = "a line long enough that it does not fit the column and scrolls across instead of wrapping";
    println!("{reply}");
}
```

> A quote, with a second line
> that runs on as quotes do.

| Construct | Built as | Note |
|---|---|---|
| Paragraph | `Rich` | runs in their own colours |
| Table | `Grid` | columns as wide as their content, cells wrapping when the table is wider than the column |
| Rule | a box | its style sizes it |

---

![a diagram of the pipeline](pipeline.png)

That is every construct."#;

/// The example's screen: its window, its library, the reply as it has streamed in so far, the pictures of its images, how far it has, how many
/// frames have passed since it came whole, and how many links were pressed.
#[derive(Bound)]
#[kind(name = "aether.example.markdown")]
pub struct Reply {
    window: WindowId,
    ui: Screen,
    document: Document,
    images: Images,
    streamed: u32,
    settled: u32,
    pressed: u32,
}

/// The example's config: the window it draws on.
#[derive(Plain)]
#[kind(name = "aether.example.markdown_config")]
pub struct ReplyConfig {
    window: WindowId,
}

/// The frame was asked for, to write as an image.
#[derive(Plain)]
#[kind(name = "aether.example.shooting")]
pub struct Shooting;

impl<H: Host> Bootstrap<H> for Reply {
    const SECTION: &'static str = "reply";
    type Settings = ();
    type Needs = (InitialWindows,);
    type Provides = ();

    async fn bootstrap(
        (): (),
        (windows,): (InitialWindows,),
        cx: &mut Bootstrapping<'_, H, Self>,
    ) -> Result<(), Unbuilt> {
        cx.begin(&(), ReplyConfig { window: windows.first() })
    }
}

aethereal_ui::style! {
    pub page {
        width: Length::Share(1.0),
        height: Length::Share(1.0),
        padding: 24,
        background: rgb(22, 21, 20),
        align_items: Alignment::Center,
    }
    pub reply { width: Length::Pixels(640.0), gap: 10 }
}

aethereal_ui::view! { Reply, ui,
    Scroll { .., style: page,
        Column { style: reply, items: self.document.blocks() as block key block.index,
            MarkdownBlock { memo, block: block, prose: self.prose(), images: self.images, link => self.open() }
        }
    }
}

impl Controls for Reply {}

#[aethereal_ui::events]
impl Reply {
    /// The style set the reply is built with: the default. Markup reads a computed value through a `&self` method.
    #[allow(clippy::unused_self)]
    fn prose(&self) -> Prose {
        Prose::default()
    }

    /// Counts the link pressed and logs its target: an example opens nothing. The target is handed as the link's
    /// event hands it.
    #[allow(clippy::needless_pass_by_value)]
    fn open(&mut self, turn: &mut Turn<'_, Self>, target: String) -> Result<(), Fault> {
        self.pressed += 1;
        turn.log(Level::Info, format_args!("link {} pressed: {target}", self.pressed));
        Ok(())
    }
}

impl ChildOf for Reply {
    type Parent = Top;
}

impl Actor for Reply {
    type Config = ReplyConfig;
    type Key = ();
    type Handles = aethereal_ui::handles![Event, On<Tick>, Answer<Read, Shooting>];
    type Depends = aethereal_ui::depends!();
    type Spawns = ();

    fn begin(config: ReplyConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        turn.to(path::<Window>(config.window)).send(&window::Subscribe)?;
        turn.subscribe::<Tick>();
        let target = Target::of::<Interface>(Frame::Main(config.window));
        let ui = Screen::begin(target, Viewport::window(1, 1, 1.0), turn)?;

        let images = match Pixels::decode(PIPELINE) {
            Ok(pixels) => Images::new(vec![(String::from("pipeline.png"), pixels)]),
            Err(undecoded) => {
                turn.log(Level::Error, format_args!("the reply's picture was not decoded: {undecoded:?}"));
                Images::default()
            }
        };

        Ok(Self {
            window: config.window,
            ui,
            document: Document::default(),
            images,
            streamed: 0,
            settled: 0,
            pressed: 0,
        })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Event> for Reply {
    type From = Anyone;

    fn handle(&mut self, event: Event, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Event::Resized(surface) = event else {
            return Ok(());
        };
        // A screen's scale is a small number that an f32 holds closely enough to lay out by.
        #[allow(clippy::cast_possible_truncation)]
        let viewport = Viewport::window(surface.width, surface.height, surface.scale as f32);
        self.ui.resize(viewport).send(turn)
    }
}

impl Receives<On<Tick>> for Reply {
    /// Streams the next characters of the reply in; once it has come and settled, reads the frame back when a file is
    /// named to write it to.
    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let streamed = usize::try_from(self.streamed).unwrap_or(usize::MAX);
        if streamed < REPLY.len() {
            let next = (streamed + CHARACTERS_PER_FRAME..=REPLY.len())
                .find(|end| REPLY.is_char_boundary(*end))
                .unwrap_or(REPLY.len());
            self.document.push_str(&REPLY[streamed..next]);
            self.streamed = u32::try_from(next).unwrap_or(u32::MAX);
            return Ok(());
        }
        self.settled += 1;
        if self.settled == SETTLE_FRAMES && shot().is_some() {
            turn.to(path::<Renderer>(())).ask(&ReadBack { of: Readable::Frame(self.window) }, &Shooting)?;
        }

        Ok(())
    }
}

impl Receives<Answer<Read, Shooting>> for Reply {
    /// Writes the frame read back as a PPM to the file named: its red, green, and blue, row by row from the top.
    fn handle(
        &mut self,
        (answered, Shooting): (Answered<Read>, Shooting),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let (Answered::Yes(Read::Read { width, height, pixels, .. }), Some(file)) = (answered, shot()) else {
            turn.log(Level::Error, "the frame was not read back");
            return Ok(());
        };
        let mut ppm = format!("P6\n{width} {height}\n255\n").into_bytes();
        for pixel in turn.read(&pixels).chunks_exact(4) {
            ppm.extend_from_slice(&pixel[..3]);
        }
        match fs::write(&file, ppm) {
            Ok(()) => turn.log(Level::Info, format_args!("the frame was written to {file}")),
            Err(failed) => turn.log(Level::Error, format_args!("the frame was not written: {failed}")),
        }

        Ok(())
    }
}

/// The file the frame is written to, as `REPLY_SHOT` names it.
fn shot() -> Option<String> {
    env::var("REPLY_SHOT").ok()
}

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

/// Launches the reply's screen on a window of its own, the renderer drawing it, on as many workers as the machine has
/// cores, at most four, a turn reading mail as large as a batch of glyphs' outlines.
fn run() -> Result<Launched<Exit>, Box<dyn Error>> {
    let workers = NonZeroU32::try_from(
        thread::available_parallelism().map_or(NonZeroUsize::MIN, |cores| cores.min(MOST_WORKERS)),
    )?;
    let engine =
        EngineSettings { workers, parts: workers, read_limit: 1 << 24, mail_bytes: 1 << 24, ..Default::default() };
    let window = WindowSettings { title: String::from(TITLE), size: [900, 1000], mode: Mode::Windowed };

    Ok(Desktop::program()
        .with_cycle::<(Tick, Prepare, Render)>()
        .with_engine(engine)
        .root_with::<Windows>(WindowsSettings(vec![window]))
        .root::<Renderer>()
        .root::<Reply>()
        .draws::<(Reply,)>()
        .launch("markdown")?)
}
