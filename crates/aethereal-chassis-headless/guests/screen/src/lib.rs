//! Spike: a guest that is one screen, written with `view!` (ADR-0056): a label of how many times its button was
//! clicked, and the button, in a column. It answers how many clicks it has counted, so a test reads its state by
//! typed mail.

use aethereal_actor::{Beginning, Fault, Portability, Turn, Upgrade, Upgrades, actor};
use aethereal_data::{Bound, Plain};
use aethereal_render::frame::Frame;
use aethereal_render::pass::Interface;
use aethereal_render::text::Font;
use aethereal_ui::prelude::*;

/// What a counter is made from: the frame it draws into.
#[derive(Plain)]
#[kind(name = "aether.guest.counter_config")]
pub struct CounterConfig {
    pub frame: Frame,
}

/// Asks a counter how many clicks it has counted.
#[derive(Plain)]
#[kind(name = "aether.guest.count_clicks")]
pub struct CountClicks;

/// How many clicks a counter has counted.
#[derive(Plain)]
#[kind(name = "aether.guest.clicks")]
pub struct Clicks {
    pub clicks: u32,
}

/// A screen: a label of its clicks over a button that adds one.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.counter")]
pub struct Counter {
    clicks: u32,
    ui: Screen,
}

aethereal_ui::style! {
    pub panel { padding: 8, gap: 4 }
    pub label { font: Font::Mono }
}

aethereal_ui::view! { Counter, ui,
    Column { style: panel,
        Text { style: label, "Clicks: {self.clicks}" }
        Row {
            Button { click => self.add(), Text { style: label, "Add" } }
        }
    }
}

impl Controls for Counter {}

impl Counter {
    fn add(&mut self, _: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.clicks += 1;
        Ok(())
    }
}

#[actor(with(aethereal_ui::screen))]
impl Counter {
    fn begin(config: CounterConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let ui = Screen::begin(Target::of::<Interface>(config.frame), Viewport::window(200, 100, 1.0), turn)?;

        Ok(Self { clicks: 0, ui })
    }

    #[request]
    fn count_clicks(&mut self, _: CountClicks) -> Result<Clicks, Fault> {
        Ok(Clicks { clicks: self.clicks })
    }
}

impl Upgrades for Counter {
    fn upgrade(&self) -> Upgrade {
        Upgrade::Now
    }
}

aethereal_actor::guest!(Counter);
