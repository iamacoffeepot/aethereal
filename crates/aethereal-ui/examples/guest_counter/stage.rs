//! The stage: a root screen on the program's first window, which spawns the counter from its kind's head once the
//! library has published `Code`, and places it in a slot that fills the window (ADR-0054, ADR-0056: Child actors).
//!
//! ```text
//! begin ──> Defaults, of no module, asked of the library ──> answered once `Code` is published
//!       ──> the counter spawned from its kind's head ──> placed: a call in the stage's list, mailed its size
//! ```

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, Level, Origin, Path, Tells, Top, Turn,
    child, path,
};
use aethereal_data::{Bound, Plain};
use aethereal_guest_counter::Counter;
use aethereal_library::{Defaulted, Defaults, Library};
use aethereal_render::Renderer;
use aethereal_render::frame::{Frame, ShapeOf};
use aethereal_render::pass::Interface;
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_ui::prelude::*;
use aethereal_window::{self as window, Event, InitialWindows, Window, WindowId};

/// The stage draws on the first window the window root opened.
impl<H: Host> Bootstrap<H> for Stage {
    const SECTION: &'static str = "stage";
    type Settings = ();
    type Needs = (InitialWindows,);
    type Provides = ();

    async fn bootstrap(
        (): (),
        (windows,): (InitialWindows,),
        cx: &mut Bootstrapping<'_, H, Self>,
    ) -> Result<(), Unbuilt> {
        cx.begin(&(), StageConfig { window: windows.first() })
    }
}

/// The stage's config: the window it draws on.
#[derive(Plain)]
#[kind(name = "aether.example.stage_config")]
pub struct StageConfig {
    pub window: WindowId,
}

/// What the stage keeps for its ask of the library: that it waits for `Code` to be published.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.example.opening")]
pub struct Opening;

/// A root screen that places the counter, a guest, in a slot that fills its window.
#[derive(Bound)]
#[kind(name = "aether.example.stage")]
pub struct Stage {
    /// The frame it and the counter draw in.
    frame: Frame,
    counter: Placing,
    ui: Screen,
}

/// Where the stage stands with the counter.
#[derive(Bound)]
enum Placing {
    /// Waiting for the library to publish `Code`, whose module makes the counter's kind.
    Waiting,
    /// Spawned from its kind's head, and placed.
    Placed(Called),
    /// Not spawned, as the stage logged: the library stopped, or the counter's kind has no head.
    Unplaced,
}

aethereal_ui::style! {
    pub fill { grow: 1 }
}

aethereal_ui::view! { Stage, ui,
    Column { style: fill,
        Column { style: fill, show: self.placed() as counter,
            child { actor: counter, area: CallArea::Inherit, style: fill }
        }
    }
}

/// What the stage's methods send: none of their own.
type StageSends = aethereal_ui::sends![];

impl Controls for Stage {
    type Sends = StageSends;
}

#[aethereal_ui::events]
impl Stage {
    /// The counter, once it is placed.
    fn placed(&self) -> Option<&Called> {
        match &self.counter {
            Placing::Placed(counter) => Some(counter),
            Placing::Waiting | Placing::Unplaced => None,
        }
    }
}

impl ChildOf for Stage {
    type Parent = Top;
}

impl Actor for Stage {
    type Config = StageConfig;
    type Key = ();
    type Handles = aethereal_ui::handles![Event, Answer<Defaulted, Opening>];
    type BeginSends = (
        Tells<Path<Window>, window::Subscribe>,
        Asks<Path<Renderer>, ShapeOf, Shaping>,
        Asks<Path<Library>, Defaults, Opening>,
    );
    type EndSends = ();

    /// Subscribes to its window for its size, begins its screen in the window's interface pass, and asks the library
    /// for defaults of no module: a library still publishing `Code` holds the ask until it has, so its answer says the
    /// counter's kind has its head.
    fn begin(config: StageConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Window>(config.window)).send(&window::Subscribe)?;
        let frame = Frame::Main(config.window);
        let ui = Screen::begin(Target::of::<Interface>(frame), Viewport::window(1, 1, 1.0), turn)?;
        turn.to(path::<Library>(())).ask(&Defaults { modules: Vec::new() }, &Opening)?;

        Ok(Self { frame, counter: Placing::Waiting, ui })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// `Code` is published: the counter is spawned from its kind's head and placed, or why not is logged.
impl Handles<Answer<Defaulted, Opening>> for Stage {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Opening): (Answered<Defaulted>, Opening),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Defaulted::Taken { .. }) = answered else {
            turn.log(
                Level::Error,
                format_args!("the library published no code to spawn the counter from: {answered:?}"),
            );
            self.counter = Placing::Unplaced;
            return Ok(());
        };
        self.counter = match turn.spawn_from::<Counter>(Origin::Head, &(), &self.frame) {
            Ok(spawned) => Placing::Placed(Called::new(child::<Counter>(()), spawned)),
            Err(refused) => {
                turn.log(Level::Error, format_args!("the counter was not spawned from its kind's head: {refused:?}"));
                Placing::Unplaced
            }
        };

        Ok(())
    }
}

/// The window's size: the stage's screen lays out to it, and mails the counter its slot's.
impl Handles<Event> for Stage {
    type Sends = aethereal_ui::Sends;

    fn handle(&mut self, event: Event, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Event::Resized(surface) = event else {
            return Ok(());
        };
        // A screen's scale is a small number that an f32 holds closely enough to lay out by.
        #[allow(clippy::cast_possible_truncation)]
        let viewport = Viewport::window(surface.width, surface.height, surface.scale as f32);
        self.ui.resize(viewport).send(turn)
    }
}
