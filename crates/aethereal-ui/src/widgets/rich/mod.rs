//! `Rich`: a text of runs in colours of their own, in its style's font and size, wrapped to the width it is given or
//! left unbroken, some of its runs links (ADR-0056: Markdown, Stock widgets). It measures, sizes, and draws as
//! [`Text`](super::Text) does, from its runs rather than one string in its style's colour. It is the library's one text
//! of several colours: a reply's paragraph, a gutter of figures, a console's tag, and, read and copied, a
//! [`Selectable`](super::Selectable)'s runs.
//!
//! One with no links raises nothing, so its event type is the unit, `Rich<()>`, and its `Default` is that one: markup
//! writes `Rich { .., runs: …, flow: Flow::Unbroken }` and leaves its links, and the event they would raise, out.
//!
//! A run that is a link shows the hand while the pointer is over it, and a press and a release of the main button both
//! on it raise `link` with its target. Which run the pointer is over is found by laying the text out as it was drawn,
//! each run's colour standing in for its place among the runs.
//!
//! Its text is shown, and selected within a scroll that selects every text (ADR-0056: Copy and paste): a press on a
//! link follows it, and a press on the panel around the rest of its text selects there.

#[cfg(test)]
mod tests;

use super::text::read::{self, joined};
use super::text::{Width, fitted, rim, scaled};
use crate::fonts::{Measuring, Unmeasured};
use crate::input::{Event, Input, Pointing, Raised, Raises, Raising};
use crate::style::Style;
use crate::widget::{Arrange, Draws, Laid, Offer, Offered, Reads, TextDraw, Widget};
use aethereal_data::Plain;
use aethereal_math::Vec2;
use aethereal_render::draw_list::Pointer;
use aethereal_render::text::{Align, Colour, LaidGlyph, Measured, Run, Wrap};
use aethereal_window::{Button, Cursor};

/// Runs of text in colours of their own, flowing as `flow` says, those `links` names raising `link` with their target
/// when pressed.
#[derive(Debug, Clone, PartialEq)]
pub struct Rich<E> {
    pub runs: Vec<Run>,
    pub links: Vec<Link>,
    pub flow: Flow,
    pub link: Raises<E>,
}

/// A run of a text that is a link: its place among the text's runs, and where it leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub run: u32,
    pub target: String,
}

/// How a text's lines break.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// At spaces, to fit the width it is given.
    Wraps,
    /// Only where it breaks them: as wide as its widest line, as code is.
    Unbroken,
}

/// Where the pointer stands with a text's links: which it is over, and which a press held began on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
#[kind(name = "aether.ui.reading")]
pub struct Reading {
    pub over: AtLink,
    pub pressed: AtLink,
}

/// A link of a text the pointer is at, by its run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
pub enum AtLink {
    #[default]
    Nothing,
    Run(u32),
}

/// A text of no runs that wraps and has no links, so it raises nothing: what markup's `..` leaves a `Rich` at. It is
/// the one `Default`, for the unit as its event type, so a `Rich` whose markup writes no event has a type rustc settles
/// on; one with links is written whole, its event the screen's.
impl Default for Rich<()> {
    fn default() -> Self {
        Self { runs: Vec::new(), links: Vec::new(), flow: Flow::Wraps, link: Raises::Nothing }
    }
}

impl<E: Event> Raising for Rich<E> {
    type Event = E;
}

impl<E> Rich<E> {
    /// Its box within `width`, as `measuring` lays it out at its style's size, with the wrap it is made with.
    fn fitted(
        &self,
        style: &Style,
        measuring: &mut Measuring<'_>,
        width: Width,
    ) -> Result<(Measured, Wrap), Unmeasured> {
        let width = match self.flow {
            Flow::Wraps => width,
            Flow::Unbroken => Width::Unbounded,
        };

        fitted(&self.runs, style, measuring, width)
    }

    /// The link of the run at `at`, in the coordinates of its box laid out at `laid`, styled `style`, measured with
    /// `measuring`, laid out as it is drawn: its runs' colours numbering them, as each glyph keeps its run's colour.
    fn link_at(&self, at: Vec2, style: &Style, laid: &Laid, measuring: &mut Measuring<'_>) -> AtLink {
        if self.links.is_empty() {
            return AtLink::Nothing;
        }
        let pad = rim(style, laid.scale);
        let width = pad.mul_add(-2.0, laid.content.width());
        let Ok((measured, wrap)) = self.fitted(style, measuring, Width::Within(width)) else {
            return AtLink::Nothing;
        };
        let origin = laid.content.min + Vec2::new(pad + shift(style, width, measured.width, wrap), pad);
        let size = style.font_size * laid.scale;
        let numbered: Vec<Run> =
            self.runs.iter().zip(0_u16..).map(|(run, at)| Run::new(&run.text, number(at))).collect();
        let (Ok(text), Ok(line)) =
            (measuring.laid(style.font, size, &numbered, (wrap, style.text_align)), measuring.line(style.font, size))
        else {
            return AtLink::Nothing;
        };
        let point = at - origin;
        let row = (point.y / line).floor();
        let on_row = |glyph: &&LaidGlyph| (glyph.origin.y / line).floor() == row;
        let mut on: Vec<_> = text.glyphs.iter().filter(on_row).collect();
        on.sort_by(|a, b| a.origin.x.total_cmp(&b.origin.x));
        let before = on.iter().rposition(|glyph| glyph.origin.x <= point.x);
        let Some(before) = before else {
            return AtLink::Nothing;
        };
        let end = on.get(before + 1).map_or_else(|| size.mul_add(0.6, on[before].origin.x), |next| next.origin.x);
        if point.x >= end {
            return AtLink::Nothing;
        }
        let run = numbered_of(on[before].colour);

        self.links.iter().find(|link| link.run == run).map_or(AtLink::Nothing, |found| AtLink::Run(found.run))
    }
}

/// The colour that numbers the run `at`: its red, which a float holds exactly for any count of runs a text has.
fn number(at: u16) -> Colour {
    Colour::opaque(f32::from(at), 0.0, 0.0)
}

/// The run a glyph laid out in the colour `colour`, as [`number`] numbers it, is of.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn numbered_of(colour: Colour) -> u32 {
    // The red of a run's number is a whole number from zero, which converts back exactly.
    colour.red.max(0.0) as u32
}

/// How far a text's lines are set in from its box's left: its widest line, `set` wide or its wrap's width, aligned in
/// `width` as `style` says.
fn shift(style: &Style, width: f32, set: f32, wrap: Wrap) -> f32 {
    let set = match wrap {
        Wrap::None => set,
        Wrap::At { width } => width,
    };
    match style.text_align {
        Align::Left => 0.0,
        Align::Centre => (width - set) / 2.0,
        Align::Right => width - set,
    }
}

impl<E: Event> Widget for Rich<E> {
    type State = Reading;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn measure(&self, style: &Style, measuring: &mut Measuring<'_>, offer: Offer) -> Vec2 {
        let pad = rim(style, measuring.scale()) * 2.0;
        let width = match offer.width {
            Offered::Exactly(width) | Offered::AtMost(width) => Width::Within(width - pad),
            Offered::MinContent => Width::Narrowest,
            Offered::MaxContent => Width::Unbounded,
        };
        let Ok((measured, _)) = self.fitted(style, measuring, width) else {
            return Vec2::ZERO;
        };

        Vec2::new(measured.width + pad, measured.height + pad)
    }

    /// One line of it, as tall as its font's line at its size, within its rim: nothing before its font's metrics come.
    fn least(&self, style: &Style, measuring: &mut Measuring<'_>) -> Vec2 {
        let scale = measuring.scale();
        let pad = rim(style, scale) * 2.0;
        let Ok(line) = measuring.line(style.font, style.font_size * scale) else {
            return Vec2::ZERO;
        };

        Vec2::new(0.0, line + pad)
    }

    /// The part of it its screen's selection holds, beneath it, then its runs.
    fn draw(&self, _: &Reading, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        read::highlight(&self.runs, self.flow, (style, laid), draws);
        let pad = rim(style, laid.scale);
        let width = pad.mul_add(-2.0, laid.content.width());
        let Ok((measured, wrap)) = self.fitted(style, draws.measuring(), Width::Within(width)) else {
            return;
        };
        draws.text(TextDraw {
            at: laid.content.min + Vec2::new(pad + shift(style, width, measured.width, wrap), pad),
            clip: laid.clip.intersection(laid.content),
            font: style.font,
            size: style.font_size * laid.scale,
            wrap,
            align: style.text_align,
            outline: scaled(style.outline, laid.scale),
            runs: self.runs.clone(),
        });
    }

    /// The hand over a link, and the default cursor elsewhere over a text with links, which passes the pointer on: a
    /// text with none is not reached.
    fn pointer(&self, reading: &Reading) -> Pointer {
        if self.links.is_empty() {
            return Pointer::Ignores;
        }
        match reading.over {
            AtLink::Run(_) => Pointer::Blocks(Cursor::Hand),
            AtLink::Nothing => Pointer::Passes(Cursor::Default),
        }
    }

    fn pointed(&self, reading: &mut Reading, pointing: Pointing, input: &mut Input<'_, '_>) {
        let style = input.style().clone();
        let laid = *input.laid();
        match pointing {
            Pointing::Moved { at } => reading.over = self.link_at(at, &style, &laid, input.measuring()),
            Pointing::Down { at, button: Button::Main, .. } => {
                reading.pressed = self.link_at(at, &style, &laid, input.measuring());
            }
            Pointing::Up { at, button: Button::Main, over } => {
                let released = self.link_at(at, &style, &laid, input.measuring());
                let pressed = reading.pressed;
                reading.pressed = AtLink::Nothing;
                let AtLink::Run(run) = pressed else {
                    return;
                };
                let Raises::Event(event) = self.link else {
                    return;
                };

                // A click is a release over the text on the run the press began on.
                let same = released == pressed;
                let clicked = over && same;
                if !clicked {
                    return;
                }
                if let Some(link) = self.links.iter().find(|link| link.run == run) {
                    input.raise(Raised::Text(event, link.target.clone()));
                }
            }
            Pointing::Left => *reading = Reading::default(),
            Pointing::Entered | Pointing::Down { .. } | Pointing::Up { .. } => {}
        }
    }

    fn reads(&self) -> Reads {
        Reads::Shown
    }

    fn read(&self) -> String {
        joined(&self.runs)
    }

    fn boundary(&self, at: Vec2, style: &Style, laid: &Laid, measuring: &mut Measuring<'_>) -> u32 {
        read::boundary(&self.runs, self.flow, at, (style, laid), measuring)
    }
}
