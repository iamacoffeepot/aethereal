//! A screen of the widget library that runs as a guest (ADR-0056, ADR-0054): a column of a text that shows a count
//! and a button that adds one to it. It is written as a native screen is, in the library's markup, and its module makes
//! one kind, its own; whoever spawns it from its kind's head places it in its own draw list, and lays it out by the
//! size it mails it. A new build published over it moves it onto its new code before its next batch, its count kept.

use aethereal_actor::{
    Actor, Anywhere, Asks, Beginning, ChildOf, Ending, Fault, Path, Portability, Turn, Upgrade, Upgrades,
};
use aethereal_data::Bound;
use aethereal_render::Renderer;
use aethereal_render::frame::{Frame, ShapeOf};
use aethereal_render::pass::Interface;
use aethereal_ui::prelude::*;

/// A count, and the screen that shows it with a button that adds to it.
#[derive(Bound, Portability)]
#[kind(name = "aether.example.counter")]
pub struct Counter {
    count: u64,
    ui: Screen,
}

aethereal_ui::style! {
    pub panel { padding: 24, gap: 16, background: rgb(28, 24, 20) }
    pub count { font_size: 32, colour: rgb(232, 217, 160) }
    pub add { padding: 12, background: rgb(70, 60, 48), radius: 6 }
    pub label { font_size: 24 }
}

aethereal_ui::view! { Counter, ui,
    Column { style: panel,
        Text { style: count, "Count: {self.count}" }
        Button { style: add, click => self.add(),
            Text { style: label, "Add one" }
        }
    }
}

/// What the counter's methods send: none of their own.
type CounterSends = aethereal_ui::sends![];

impl Controls for Counter {
    type Sends = CounterSends;
}

#[aethereal_ui::events]
impl Counter {
    fn add(&mut self, _: &mut Turn<'_, Self, CounterSends>) -> Result<(), Fault> {
        self.count += 1;
        Ok(())
    }
}

/// Spawned by whoever places it, from its kind's head.
impl ChildOf for Counter {
    type Parent = Anywhere;
}

impl Actor for Counter {
    /// The frame it draws in, in that frame's interface pass.
    type Config = Frame;
    type Key = ();
    type Handles = aethereal_ui::handles![];
    type BeginSends = (Asks<Path<Renderer>, ShapeOf, Shaping>,);
    type EndSends = ();

    /// Begins its screen at a pixel, until whoever places it mails it its size.
    fn begin(frame: Frame, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let ui = Screen::begin(Target::of::<Interface>(frame), Viewport::window(1, 1, 1.0), turn)?;

        Ok(Self { count: 0, ui })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// It holds nothing a batch must finish, so it moves as soon as it is asked.
impl Upgrades for Counter {
    fn upgrade(&self) -> Upgrade {
        Upgrade::Now
    }
}

aethereal_actor::guest!(Counter);
