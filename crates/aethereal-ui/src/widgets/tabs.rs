//! `Tabs`: a row of headers over one panel (ADR-0056: Stock widgets). Which tab is open is the screen's, as game state
//! is: `Tabs` is given it, `selected`, and raises its `change` with the tab chosen, counted from zero; the screen's
//! method keeps it, and the view builds the panel of the tab it names, alone, as a `Tab` at that tab's place, so each
//! panel's widgets keep keys of their own. `Tabs` never changes which tab is open itself.
//!
//! It keeps room above its panel for its headers, one line of its style's font each, side by side: the open one
//! filled faintly and underlined in its style's colour, and the one the pointer is over tinted. A press and a release
//! on one header chooses it. It is one stop of the focus, its ring around the open header; with focus, the arrows
//! choose the tab beside the open one, round from the last to the first, and Home and End the first and the last.
//!
//! ```text
//! Tabs { selected: self.tab, change => self.open(),     Tabs { titles: ["Items", "Skills"], selected, change }
//!     Tab { title: "Items", … }                 ──>     Tab { title: "Items" } at Place(0), when selected is 0
//!     Tab { title: "Skills", … }                        Tab { title: "Skills" } at Place(1), when selected is 1
//! }
//! ```

use super::press::{HOVERED, RING, RING_WIDTH};
use crate::fonts::{Measuring, Unmeasured};
use crate::input::{Event, Hears, Input, Key, Pointing, Raised, Raising, Used};
use crate::style::{Edges, Style};
use crate::widget::{Arrange, Axis, Draws, Laid, Quad, Sample, Stateless, TextDraw, Widget};
use aethereal_data::Plain;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::draw_list::Pointer;
use aethereal_render::text::{Align, Colour, Outline, Run, Wrap};
use aethereal_window::{Button, Cursor};

/// The keys that choose a tab while the headers have focus.
const CHOOSES: &[&str] = &["ArrowLeft", "ArrowRight", "Home", "End"];

/// How far a header's title is inset from its edges, in logical pixels.
const INSET: f32 = 6.0;

/// How thick the open header's underline is, in logical pixels.
const UNDERLINE: f32 = 2.0;

/// How strong the open header's fill is, of its style's colour's alpha.
const OPEN_STRENGTH: f32 = 0.15;

/// Headers titled `titles`, the one at `selected` open, raising `change` with the tab chosen; its child the open tab's
/// panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tabs<E> {
    pub titles: Vec<String>,
    pub selected: u32,
    pub change: E,
}

/// One tab's panel, titled `title`, its children one after another down: what `Tabs` shows below its headers while
/// its tab is open. Its title is what its header shows, which the markup's macro hands `Tabs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    pub title: String,
}

/// Which header the pointer is over, and which a press of the main button on is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
#[kind(name = "aether.ui.tabs")]
pub struct Tabbing {
    pub hovered: Header,
    pub pressed: Header,
}

/// One of a tab strip's headers, or none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
pub enum Header {
    #[default]
    Nowhere,
    At(u32),
}

impl<E> Tabs<E> {
    /// Headers titled `titles`, the one at `selected` open, raising `change`.
    #[must_use]
    pub fn new(titles: &[&str], selected: u32, change: E) -> Self {
        Self { titles: titles.iter().map(|title| String::from(*title)).collect(), selected, change }
    }

    /// How tall its headers are, in pixels: a line of its style's font and the inset above and below it.
    fn height(style: &Style, measuring: &mut Measuring<'_>) -> Result<f32, Unmeasured> {
        let scale = measuring.scale();
        let line = [Run::new(" ", style.colour)];
        let measured = measuring.text(style.font, style.font_size * scale, &line, Wrap::None)?;

        Ok((INSET * scale).mul_add(2.0, measured.height))
    }

    /// Each header's box, laid in `rect`, its border box, styled `style`: side by side along its top, inside its
    /// border, each as wide as its title and the inset either side.
    fn headers(&self, style: &Style, rect: Rect2, measuring: &mut Measuring<'_>) -> Result<Vec<Rect2>, Unmeasured> {
        let scale = measuring.scale();
        let height = Self::height(style, measuring)?;
        let edge = style.border.width * scale;
        let mut x = rect.min.x + edge;
        let top = rect.min.y + edge;
        let mut headers = Vec::with_capacity(self.titles.len());
        for title in &self.titles {
            let runs = [Run::new(title, style.colour)];
            let measured = measuring.text(style.font, style.font_size * scale, &runs, Wrap::None)?;
            let width = (INSET * scale).mul_add(2.0, measured.width);
            headers.push(Rect2::from_origin_size(Vec2::new(x, top), Vec2::new(width, height)));
            x += width;
        }

        Ok(headers)
    }

    /// The header under `at`, laid in `rect` and styled `style`.
    fn under(&self, at: Vec2, style: &Style, rect: Rect2, measuring: &mut Measuring<'_>) -> Header {
        let headers = self.headers(style, rect, measuring).unwrap_or_default();
        let found = headers.iter().position(|header| header.contains_point(at));

        found.map_or(Header::Nowhere, |at| Header::At(index(at)))
    }
}

/// A header's place, as its tabs count it.
fn index(at: usize) -> u32 {
    u32::try_from(at).unwrap_or(u32::MAX)
}

impl<E: Event> Tabs<E> {
    /// Raises `change` with `tab`, when it is not the open one.
    fn choose(&self, tab: u32, input: &mut Input<'_, '_>) {
        let other = tab != self.selected;
        if other {
            input.raise(Raised::Index(self.change, tab));
        }
    }
}

impl<E: Event> Raising for Tabs<E> {
    type Event = E;
}

impl<E: Event> Widget for Tabs<E> {
    type State = Tabbing;

    fn arrange(&self) -> Arrange {
        Arrange::Flex(Axis::Column)
    }

    /// Its headers' height, above its panel.
    fn room(&self, style: &Style, measuring: &mut Measuring<'_>) -> Edges<f32> {
        let top = Self::height(style, measuring).unwrap_or(0.0);

        Edges { top, ..Edges::all(0.0) }
    }

    fn draw(&self, tabbing: &Tabbing, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let focused = draws.focused();
        let Ok(headers) = self.headers(style, laid.rect, draws.measuring()) else {
            return;
        };
        let quad = |rect: Rect2, colour: Colour| Quad {
            rect,
            clip: laid.clip,
            colour,
            border_width: 0.0,
            border_colour: colour,
            radius: 0.0,
            sample: Sample::Solid,
        };
        for (at, header) in headers.iter().enumerate() {
            let tab = index(at);
            let open = tab == self.selected;
            let hovered = tabbing.hovered == Header::At(tab);
            if open {
                let underline =
                    Rect2 { min: Vec2::new(header.min.x, UNDERLINE.mul_add(-laid.scale, header.max.y)), ..*header };
                draws.quad(quad(*header, Colour { alpha: style.colour.alpha * OPEN_STRENGTH, ..style.colour }));
                draws.quad(quad(underline, style.colour));
            }
            if hovered {
                draws.quad(quad(*header, HOVERED));
            }
            let ringed = open && focused;
            if ringed {
                draws.quad(Quad {
                    border_width: RING_WIDTH * laid.scale,
                    border_colour: RING,
                    ..quad(*header, Colour::CLEAR)
                });
            }
            let inset = INSET * laid.scale;
            draws.text(TextDraw {
                at: header.min + Vec2::new(inset, inset),
                clip: laid.clip.intersection(*header),
                font: style.font,
                size: style.font_size * laid.scale,
                wrap: Wrap::None,
                align: Align::Left,
                outline: Outline::None,
                runs: vec![Run::new(&self.titles[at], style.colour)],
            });
        }
    }

    /// The hand over a header, and the default cursor over the panel's empty space, which is solid to what is beneath.
    fn pointer(&self, tabbing: &Tabbing) -> Pointer {
        let cursor = match tabbing.hovered {
            Header::At(_) => Cursor::Hand,
            Header::Nowhere => Cursor::Default,
        };

        Pointer::Blocks(cursor)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn hears(&self) -> Hears {
        Hears::Keys(CHOOSES)
    }

    fn pointed(&self, tabbing: &mut Tabbing, pointing: Pointing, input: &mut Input<'_, '_>) {
        let style = input.style().clone();
        let rect = input.laid().rect;
        match pointing {
            Pointing::Entered | Pointing::Left => tabbing.hovered = Header::Nowhere,
            Pointing::Moved { at } => tabbing.hovered = self.under(at, &style, rect, input.measuring()),
            Pointing::Down { at, button: Button::Main } => {
                tabbing.pressed = self.under(at, &style, rect, input.measuring());
            }
            Pointing::Up { at, button: Button::Main, over } => {
                let pressed = tabbing.pressed;
                tabbing.pressed = Header::Nowhere;
                let released = self.under(at, &style, rect, input.measuring());
                let Header::At(tab) = pressed else {
                    return;
                };
                let clicked = over && released == pressed;
                if clicked {
                    self.choose(tab, input);
                }
            }
            Pointing::Down { .. } | Pointing::Up { .. } => {}
        }
    }

    fn key(&self, _: &mut Tabbing, key: &Key, input: &mut Input<'_, '_>) -> Used {
        let count = index(self.titles.len());
        let none = count == 0;
        if none {
            return Used::Passed;
        }
        let tab = match key.code.as_str() {
            "ArrowRight" => (self.selected.min(count - 1) + 1) % count,
            "ArrowLeft" => (self.selected.min(count - 1) + count - 1) % count,
            "Home" => 0,
            "End" => count - 1,
            _ => return Used::Passed,
        };
        self.choose(tab, input);

        Used::Used
    }
}

impl Widget for Tab {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Flex(Axis::Column)
    }

    fn draw(&self, _: &Stateless, _: &Style, _: &Laid, _: &mut Draws<'_, '_>) {}
}
