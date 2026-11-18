//! Drag and drop between two screens in one window, in an engine linked to a hub (ADR-0056: Drag and drop is one
//! path). A root screen places two shelves, sibling child actors of one type, one at each side of the window with a
//! gap of nothing between them. Each shelf is a keyed list of fruit and of stones, each item dragged by a press and a
//! move: a release over the other shelf mails it the drop, which it takes, and its library answers that it did, so the
//! source lets the item go; the right shelf takes no stones, so its library refuses one and the source puts it back;
//! and a release over the gap, where nothing is, puts the item back too. A release over the item's own shelf lets it go
//! and takes it in again, at the end of its list. Each shelf logs what it took, what it let go, and what it put back.
//!
//! ```text
//! press on an item ──> move past DRAG_THRESHOLD ──> release over the other shelf ──> Dropped, mailed to it
//!     ├─> it takes the kind ──> its take_* runs ──> DropAnswered Accepted, replied ──> the source's let_go_* lets it go
//!     ├─> it takes no stones ──> DropAnswered Refused, replied ──> the source's keep_stone puts it back
//!     └─> release over the gap ──> the source's keep_* puts it back
//! ```
//!
//! The engine hands the hub `capture_frame` (ADR-0043), `send_input` (ADR-0046), `actor_logs` (ADR-0044), and the
//! window tools (ADR-0047). With a hub from the `aethereal-hub` crate running, ask its `mint_token` tool for a
//! token, which answers the arguments to start this engine with, then run `cargo run --release -p aethereal-overlay
//! --example drag -- <those arguments> --name drag`; or spawn it from the hub's store. `--size 1920x1080` sizes its
//! window and `--mode` shows it so as it opens; `--help` lists every flag it reads, each also read from its `DRAG_`
//! variable.

#![allow(clippy::print_stdout, clippy::print_stderr)] // An example prints how it ended.

use aethereal_actor::{
    Actor, Anyone, Beginning, ChildOf, Ending, Fault, Handles, Level, Prepare, Render, Spawn, Tick, Top, Turn, child,
    path,
};
use aethereal_chassis_desktop::{Desktop, WindowSettings, WindowsSettings};
use aethereal_chassis_headless::Exit;
use aethereal_chassis_headless::native::{Launched, NativeLayers};
use aethereal_data::{Bound, KeyName, Plain, TypedKey};
use aethereal_files::Files;
use aethereal_link::{Link, LinkSettings};
use aethereal_pace::{PaceTool, Pacer};
use aethereal_render::frame::Frame;
use aethereal_render::pass::Interface;
use aethereal_render::{CaptureTool, Renderer};
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Unbuilt};
use aethereal_tcp::Tcp;
use aethereal_tools::{LogsTool, Toolbox, Tools};
use aethereal_ui::prelude::*;
use aethereal_window::{self as window, Event, InitialWindows, InputTool, Mode, Window, WindowId, WindowTool, Windows};
use core::error::Error;
use core::num::{NonZeroU32, NonZeroUsize};
use std::process::ExitCode;
use std::thread;

/// The most workers the program runs on.
const MOST_WORKERS: NonZeroUsize = NonZeroUsize::new(4).expect("4 is not zero");

/// The window's title unless `--title` names another.
const TITLE: &str = "Aethereal drag and drop";

/// The most bytes of a frame on the link: room for an image of a large frame.
const MOST_LINK_BYTES: u32 = 16 << 20;

/// The left shelf's key, as a path writes it.
const LEFT: KeyName = KeyName::new("left");

/// The right shelf's key, as a path writes it.
const RIGHT: KeyName = KeyName::new("right");

/// A fruit, by its number: what a drag of one carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.example.fruit")]
pub struct Fruit(u32);

/// A stone, by its number: what a drag of one carries, a kind apart from a fruit's, so a shelf that takes only fruit
/// refuses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.example.stone")]
pub struct Stone(u32);

/// Which side of the window a shelf stands at: its key. The left takes fruit and stones, the right only fruit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.example.hand")]
pub enum Hand {
    Left,
    Right,
}

impl TypedKey for Hand {
    fn to_name(&self) -> KeyName {
        match self {
            Self::Left => LEFT,
            Self::Right => RIGHT,
        }
    }

    fn from_name(name: &KeyName) -> Option<Self> {
        match name {
            name if *name == LEFT => Some(Self::Left),
            name if *name == RIGHT => Some(Self::Right),
            _ => None,
        }
    }
}

/// The root screen: its window, the two shelves it places, and its library.
#[derive(Bound)]
#[kind(name = "aether.example.shelves")]
pub struct Shelves {
    window: WindowId,
    left: Called,
    right: Called,
    ui: Screen,
}

/// The root screen's config: the window it draws on.
#[derive(Plain)]
#[kind(name = "aether.example.shelves_config")]
pub struct ShelvesConfig {
    window: WindowId,
}

/// A shelf was not spawned, so the root screen does not begin.
#[derive(Plain)]
#[kind(name = "aether.example.no_shelf")]
pub struct NoShelf;

/// A shelf: which side it stands at, its fruit and its stones, and its library.
#[derive(Bound)]
#[kind(name = "aether.example.shelf")]
pub struct Shelf {
    hand: Hand,
    fruit: Vec<Fruit>,
    stones: Vec<Stone>,
    ui: Screen,
}

/// A shelf's config: the frame it draws in and the side it stands at, which is also its key.
#[derive(Plain)]
#[kind(name = "aether.example.shelf_config")]
pub struct ShelfConfig {
    frame: Frame,
    hand: Hand,
}

impl<H: Host> Bootstrap<H> for Shelves {
    const SECTION: &'static str = "shelves";
    type Settings = ();
    type Needs = (InitialWindows,);
    type Provides = ();

    async fn bootstrap(
        (): (),
        (windows,): (InitialWindows,),
        cx: &mut Bootstrapping<'_, H, Self>,
    ) -> Result<(), Unbuilt> {
        cx.begin(&(), ShelvesConfig { window: windows.first() })
    }
}

aethereal_ui::style! {
    pub page {
        width: Length::Share(1.0),
        height: Length::Share(1.0),
        padding: 24,
        background: rgb(22, 21, 20),
        justify_content: Distribute::SpaceBetween,
    }
    pub slot { width: 280, height: Length::Share(1.0) }
    pub shelf { width: Length::Share(1.0), height: Length::Share(1.0), padding: 16, gap: 8, background: rgb(44, 38, 32) }
    pub heading { font_size: 18, colour: rgb(232, 217, 160) }
    pub list { gap: 6 }
    pub bin { padding: 8, gap: 6, min_height: 120, background: rgb(64, 56, 48) }
    pub fruit_item { padding: 8, background: rgb(84, 122, 64) }
    pub stone_item { padding: 8, background: rgb(110, 110, 116) }
    pub label { font_size: 16, colour: rgb(240, 240, 240) }
}

aethereal_ui::view! { Shelves, ui,
    Row { style: page,
        child { actor: self.left, area: CallArea::Inherit, style: slot }
        child { actor: self.right, area: CallArea::Inherit, style: slot }
    }
}

aethereal_ui::view! { Shelf, ui,
    Column { style: shelf, drop => self.take_fruit(),
        Text { style: heading, "{self.title()}" }
        Column { style: list, items: self.fruit as fruit key fruit.0,
            Stack {
                style: fruit_item,
                drag: fruit,
                drop_accepted => self.let_go_fruit(),
                drop_outside => self.keep_fruit(),
                Text { style: label, "fruit {fruit.0}" }
            }
        }
        Column { style: bin, show: self.takes_stones(), drop => self.take_stone(),
            Text { style: heading, "stones" }
            Column { style: list, items: self.stones as stone key stone.0,
                Stack {
                    style: stone_item,
                    drag: stone,
                    drop_accepted => self.let_go_stone(),
                    drop_outside => self.keep_stone(),
                    Text { style: label, "stone {stone.0}" }
                }
            }
        }
    }
}

impl Controls for Shelves {}

#[aethereal_ui::events]
impl Shelves {}

impl Controls for Shelf {}

#[aethereal_ui::events]
impl Shelf {
    /// The shelf's heading: its side, and what it takes.
    fn title(&self) -> &'static str {
        match self.hand {
            Hand::Left => "left: fruit and stones",
            Hand::Right => "right: fruit only",
        }
    }

    /// Whether it takes stones: only the left does, so the right has no bin for them and refuses one dropped on it.
    fn takes_stones(&self) -> bool {
        self.hand == Hand::Left
    }

    /// Takes `fruit`, dropped on it: the library answers the shelf it came from that it was taken. One dropped back on
    /// its own shelf was let go of first, so it moves to the end.
    fn take_fruit(&mut self, turn: &mut Turn<'_, Self>, fruit: Fruit) -> Result<(), Fault> {
        self.fruit.push(fruit);
        turn.log(Level::Info, format_args!("took fruit {}", fruit.0));
        Ok(())
    }

    /// Takes `stone`, dropped on its bin: the library answers the shelf it came from that it was taken. One dropped
    /// back on its own shelf was let go of first, so it moves to the end.
    fn take_stone(&mut self, turn: &mut Turn<'_, Self>, stone: Stone) -> Result<(), Fault> {
        self.stones.push(stone);
        turn.log(Level::Info, format_args!("took stone {}", stone.0));
        Ok(())
    }

    /// Lets go of `fruit`, which a shelf took.
    fn let_go_fruit(&mut self, turn: &mut Turn<'_, Self>, fruit: Fruit) -> Result<(), Fault> {
        self.fruit.retain(|kept| *kept != fruit);
        turn.log(Level::Info, format_args!("let fruit {} go", fruit.0));
        Ok(())
    }

    /// Lets go of `stone`, which a shelf took.
    fn let_go_stone(&mut self, turn: &mut Turn<'_, Self>, stone: Stone) -> Result<(), Fault> {
        self.stones.retain(|kept| *kept != stone);
        turn.log(Level::Info, format_args!("let stone {} go", stone.0));
        Ok(())
    }

    /// Puts `fruit` back, which nothing under its release took: it never left the shelf.
    #[allow(clippy::unused_self)]
    fn keep_fruit(&mut self, turn: &mut Turn<'_, Self>, fruit: Fruit) -> Result<(), Fault> {
        turn.log(Level::Info, format_args!("put fruit {} back", fruit.0));
        Ok(())
    }

    /// Puts `stone` back, which nothing under its release took, or the shelf it was dropped on refused.
    #[allow(clippy::unused_self)]
    fn keep_stone(&mut self, turn: &mut Turn<'_, Self>, stone: Stone) -> Result<(), Fault> {
        turn.log(Level::Info, format_args!("put stone {} back", stone.0));
        Ok(())
    }
}

impl ChildOf for Shelves {
    type Parent = Top;
}

impl ChildOf for Shelf {
    type Parent = Shelves;
}

impl Actor for Shelves {
    type Config = ShelvesConfig;
    type Key = ();
    type Handles = aethereal_ui::handles![Event];
    type Depends = aethereal_ui::depends!();
    type Spawns = (Shelf,);

    fn begin(config: ShelvesConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        turn.to(path::<Window>(config.window)).send(&window::Subscribe)?;
        let frame = Frame::Main(config.window);
        let (Ok(left), Ok(right)) = (
            turn.spawn::<Shelf>(&Hand::Left, ShelfConfig { frame, hand: Hand::Left }),
            turn.spawn::<Shelf>(&Hand::Right, ShelfConfig { frame, hand: Hand::Right }),
        ) else {
            return Err(Fault::new(&NoShelf)?);
        };
        let ui = Screen::begin(Target::of::<Interface>(frame), Viewport::window(1, 1, 1.0), turn)?;

        Ok(Self {
            window: config.window,
            left: Called::new(child::<Shelf>(Hand::Left), left),
            right: Called::new(child::<Shelf>(Hand::Right), right),
            ui,
        })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Event> for Shelves {
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

impl Actor for Shelf {
    type Config = ShelfConfig;
    type Key = Hand;
    type Handles = aethereal_ui::handles![];
    type Depends = aethereal_ui::depends!();
    type Spawns = ();

    /// The left shelf begins with three fruit and two stones, the right with two fruit.
    fn begin(config: ShelfConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let ui = Screen::begin(Target::of::<Interface>(config.frame), Viewport::window(1, 1, 1.0), turn)?;
        let (fruit, stones) = match config.hand {
            Hand::Left => (vec![Fruit(1), Fruit(2), Fruit(3)], vec![Stone(1), Stone(2)]),
            Hand::Right => (vec![Fruit(4), Fruit(5)], Vec::new()),
        };

        Ok(Self { hand: config.hand, fruit, stones, ui })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
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

/// Launches the shelves on a window of their own, the link to the hub beside them, and a toolbox of the renderer's
/// capture, the window's input and window tools, and the logs' tool offered to the tools the link hands the hub, on as
/// many workers as the machine has cores, at most four, with what its command line and environment set over that.
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
    let window = WindowSettings { title: String::from(TITLE), size: [800, 600], mode: Mode::Windowed };

    Ok(Desktop::program()
        .with_cycle::<(Tick, Prepare, Render)>()
        .with_engine(engine)
        .root_with::<Windows>(WindowsSettings(vec![window]))
        .root::<Renderer>()
        .root::<Pacer>()
        .root::<Shelves>()
        .root::<Tools>()
        .root::<Files>()
        .root::<Tcp>()
        .root_with::<Link>(link)
        .root::<Toolbox<(CaptureTool, LogsTool, InputTool, WindowTool, PaceTool), Tools>>()
        .draws::<(Shelves,)>()
        .flags(|flags| {
            flags.flag("size", 's', WindowsSettings::FIRST_SIZE).flag("mode", 'm', WindowsSettings::FIRST_MODE)
        })
        .launch("drag")?)
}
