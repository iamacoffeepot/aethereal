//! `Scroll`: a box that shows part of what it holds and slides through the rest (ADR-0056: Stock widgets). Its children
//! run one after another along its axis, never shrunk to fit, cut to its content box, and slid by its offset, which is
//! its own state, kept by its key; the library clamps it to what it can scroll as it lays the screen out.
//!
//! The wheel slides it while the pointer is over it or what it holds, unless a scroll within it can still move that
//! way. It keeps room along its far side for its bar, where a thumb as long as the share of what it holds it shows
//! slides as it does: a press on the thumb drags it, wherever the pointer goes while the press is held, as the window
//! keeps sending moves to what a press reached (ADR-0055: capture), and a press elsewhere on the bar jumps the thumb's
//! middle there and drags it. With focus, or with a widget within it focused, Page Up and Page Down slide it a box's
//! length, and Home and End to its ends.
//!
//! Given `items`, it builds only the items it shows: its lines are [`Lines::Window`], made by [`Rows`](crate::Rows)
//! as the view asks the screen which items to build.

use super::press::ring;
use crate::fonts::Measuring;
use crate::input::{Hears, Input, Key, Pointing, Used};
use crate::style::{Edges, Style};
use crate::widget::{Arrange, Axis, Draws, Laid, Lines, Quad, Sample, Widget};
use aethereal_data::Plain;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::draw_list::Pointer;
use aethereal_render::text::Colour;
use aethereal_window::{Button, Cursor};

/// The keys that slide a scroll with focus, or around a focused widget.
const PAGES: &[&str] = &["PageUp", "PageDown", "Home", "End"];

/// How wide a scroll's bar is, in logical pixels.
const BAR: f32 = 8.0;

/// How short a scroll's thumb may get, in logical pixels.
const LEAST_THUMB: f32 = 16.0;

/// How strong a scroll's bar is, of its style's colour's alpha.
const TRACK_STRENGTH: f32 = 0.15;

/// How strong a scroll's thumb is, of its style's colour's alpha, at rest and while dragged.
const THUMB_STRENGTH: f32 = 0.5;
const DRAGGED_STRENGTH: f32 = 0.8;

/// What it holds, along `axis`, slid through: its children, or, as `lines` says, the items of a list it built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scroll {
    pub axis: Axis,
    pub lines: Lines,
}

/// How far a scroll has slid what it holds, in pixels, and whether a press on its bar drags its thumb.
#[derive(Debug, Clone, Copy, PartialEq, Default, Plain)]
#[kind(name = "aether.ui.scroll")]
pub struct Scrolling {
    pub offset: f32,
    pub drag: Drag,
}

/// Whether a press drags a scroll's thumb.
#[derive(Debug, Clone, Copy, PartialEq, Default, Plain)]
pub enum Drag {
    #[default]
    Idle,
    /// The thumb is dragged, held `grip` pixels along it from its start.
    Thumb { grip: f32 },
}

impl Scroll {
    /// A scroll down through its children.
    #[must_use]
    pub const fn down() -> Self {
        Self { axis: Axis::Column, lines: Lines::Whole }
    }

    /// A scroll across through its children.
    #[must_use]
    pub const fn across() -> Self {
        Self { axis: Axis::Row, lines: Lines::Whole }
    }

    /// It, its children the items of a list its `lines` name.
    #[must_use]
    pub fn list(self, lines: Lines) -> Self {
        Self { lines, ..self }
    }

    /// Its bar, laid at `laid` and styled `style`, with its thumb where `offset` puts it.
    fn bar(&self, offset: f32, style: &Style, laid: &Laid) -> Bar {
        let axis = self.axis;
        let edge = style.border.width * laid.scale;
        let room = BAR * laid.scale;
        let view = laid.content;
        let track = match axis {
            Axis::Column => Rect2 {
                min: Vec2::new(laid.rect.max.x - edge - room, view.min.y),
                max: Vec2::new(laid.rect.max.x - edge, view.max.y),
            },
            Axis::Row => Rect2 {
                min: Vec2::new(view.min.x, laid.rect.max.y - edge - room),
                max: Vec2::new(view.max.x, laid.rect.max.y - edge),
            },
        };
        let long = axis.along(track.size());
        let travel = laid.travel(axis);
        let shown = axis.along(view.size());
        let whole = shown + travel;
        let share = if whole > 0.0 {
            shown / whole
        } else {
            1.0
        };
        let thumb_length = (long * share).max(LEAST_THUMB * laid.scale).min(long);
        let slack = long - thumb_length;
        let along = if travel > 0.0 {
            offset / travel * slack
        } else {
            0.0
        };
        let start = track.min + axis.vector(along);
        let thumb_size = match axis {
            Axis::Column => Vec2::new(track.width(), thumb_length),
            Axis::Row => Vec2::new(thumb_length, track.height()),
        };
        let thumb = Rect2::from_origin_size(start, thumb_size);

        Bar { axis, track, thumb, slack, travel }
    }
}

/// A scroll's bar: its track along its far side, its thumb, how far the thumb can move along the track, and how far
/// the scroll can slide.
struct Bar {
    axis: Axis,
    track: Rect2,
    thumb: Rect2,
    slack: f32,
    travel: f32,
}

impl Bar {
    /// The offset that puts the thumb's start `start` pixels along the axis, in the coordinates of its track.
    fn offset_at(&self, start: f32) -> f32 {
        let moves = self.slack > 0.0;
        if !moves {
            return 0.0;
        }
        let along = (start - self.axis.along(self.track.min)) / self.slack;

        along.clamp(0.0, 1.0) * self.travel
    }
}

impl Widget for Scroll {
    type State = Scrolling;

    fn arrange(&self) -> Arrange {
        Arrange::Scroll(self.axis)
    }

    /// Its bar's width along its far side: the right of a scroll down, the bottom of one across.
    fn room(&self, _: &Style, measuring: &mut Measuring<'_>) -> Edges<f32> {
        let room = BAR * measuring.scale();
        match self.axis {
            Axis::Column => Edges { right: room, ..Edges::all(0.0) },
            Axis::Row => Edges { bottom: room, ..Edges::all(0.0) },
        }
    }

    fn lines(&self) -> &Lines {
        &self.lines
    }

    fn offset(&self, scrolling: &Scrolling) -> f32 {
        scrolling.offset
    }

    fn scroll_to(&self, scrolling: &mut Scrolling, offset: f32) {
        scrolling.offset = offset;
    }

    /// Its bar, faint, and its thumb over it, while it holds more than it shows; and its focus ring.
    fn draw(&self, scrolling: &Scrolling, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let bar = self.bar(scrolling.offset, style, laid);
        let scrolls = bar.travel > 0.0;
        if scrolls {
            let strength = match scrolling.drag {
                Drag::Idle => THUMB_STRENGTH,
                Drag::Thumb { .. } => DRAGGED_STRENGTH,
            };
            let faint = Colour { alpha: style.colour.alpha * TRACK_STRENGTH, ..style.colour };
            let strong = Colour { alpha: style.colour.alpha * strength, ..style.colour };
            let round = BAR * laid.scale / 2.0;
            let quad = |rect: Rect2, colour: Colour| Quad {
                rect,
                clip: laid.clip,
                colour,
                border_width: 0.0,
                border_colour: colour,
                radius: round,
                sample: Sample::Solid,
            };
            draws.quad(quad(bar.track, faint));
            draws.quad(quad(bar.thumb, strong));
        }
        ring(style, laid, draws);
    }

    fn pointer(&self, scrolling: &Scrolling) -> Pointer {
        let cursor = match scrolling.drag {
            Drag::Idle => Cursor::Default,
            Drag::Thumb { .. } => Cursor::Grabbing,
        };

        Pointer::Blocks(cursor)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn hears(&self) -> Hears {
        Hears::Keys(PAGES)
    }

    fn pointed(&self, scrolling: &mut Scrolling, pointing: Pointing, input: &mut Input<'_, '_>) {
        let bar = self.bar(scrolling.offset, input.style(), input.laid());
        let axis = self.axis;
        match (pointing, scrolling.drag) {
            (Pointing::Down { at, button: Button::Main }, _) if bar.track.contains_point(at) => {
                let on_thumb = bar.thumb.contains_point(at);
                let grip = if on_thumb {
                    axis.along(at - bar.thumb.min)
                } else {
                    axis.along(bar.thumb.size()) / 2.0
                };
                scrolling.drag = Drag::Thumb { grip };
                scrolling.offset = bar.offset_at(axis.along(at) - grip);
            }
            (Pointing::Moved { at }, Drag::Thumb { grip }) => scrolling.offset = bar.offset_at(axis.along(at) - grip),
            (Pointing::Up { button: Button::Main, .. }, _) => scrolling.drag = Drag::Idle,
            (
                Pointing::Entered
                | Pointing::Left
                | Pointing::Moved { .. }
                | Pointing::Down { .. }
                | Pointing::Up { .. },
                _,
            ) => {}
        }
    }

    /// Page Up and Page Down slide it a content box's length back and on, Home to its start and End to its end.
    fn key(&self, scrolling: &mut Scrolling, key: &Key, input: &mut Input<'_, '_>) -> Used {
        let laid = input.laid();
        let page = self.axis.along(laid.content.size());
        let travel = laid.travel(self.axis);
        let to = match key.code.as_str() {
            "PageDown" => scrolling.offset + page,
            "PageUp" => scrolling.offset - page,
            "Home" => 0.0,
            "End" => travel,
            _ => return Used::Passed,
        };
        scrolling.offset = to.clamp(0.0, travel);

        Used::Used
    }
}
