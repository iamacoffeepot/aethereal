//! A widget's style, resolved (ADR-0056: Styles are Rust items): every property a style sets, as concrete values.
//! `style!` makes each style a function that starts from one other style, or none, and sets properties over it, and an
//! element names one, or extends one for itself alone; the runtime takes the result as it is: nothing is looked up,
//! cascaded, or inherited here.
//!
//! Sizes are logical pixels. The screen multiplies them by its UI scale as it lays out, so one scale enlarges
//! everything without touching a style.

mod convert;

pub use convert::{Parent, flow};

use aethereal_render::text::{Align, Colour, Font, Outline};

/// Every property a widget is styled by: its box, how it lays its children out, where it stands in its parent's grid,
/// how it looks, and its text. Which way a flex container runs is not here: it is the widget, `Row` or `Column`, so a
/// style cannot say otherwise than the element.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub width: Length,
    pub height: Length,
    pub min_width: Length,
    pub min_height: Length,
    pub max_width: Length,
    pub max_height: Length,
    pub margin: Edges<Length>,
    pub padding: Edges<Space>,
    /// The space between rows, and between columns, of a flex or grid container's children.
    pub gap: Gap,
    /// How much of a flex container's free space this child takes, against its siblings'.
    pub grow: f32,
    /// How much this child gives up when its flex container is short of space, against its siblings'.
    pub shrink: f32,
    /// This child's size along its flex container's axis before it grows or shrinks.
    pub basis: Length,
    /// Whether a flex container's children wrap onto more lines when one is full.
    pub wrap: bool,
    /// How a container shares its free space out along its main axis, a grid's along its rows.
    pub justify_content: Distribute,
    /// How a container's lines share its cross axis, a grid's its columns.
    pub align_content: Distribute,
    /// Where a container sets its children across its cross axis, a grid's in their cells' height.
    pub align_items: Alignment,
    /// Where a grid sets its children in their cells' width.
    pub justify_items: Alignment,
    /// Where this child sets itself across its container's cross axis, over its container's `align_items`.
    pub align_self: SelfAlign,
    /// Where this child sets itself in its grid cell's width, over its grid's `justify_items`.
    pub justify_self: SelfAlign,
    /// A grid's columns, in order.
    pub columns: Vec<Track>,
    /// A grid's rows, in order: rows past them are as tall as their content.
    pub rows: Vec<Track>,
    /// Which of its grid's columns this child stands in.
    pub column: Placed,
    /// Which of its grid's rows this child stands in.
    pub row: Placed,
    /// What fills its box beneath its content: clear for nothing.
    pub background: Colour,
    /// A line around its box, inside it: it takes room from the content as padding does.
    pub border: Border,
    /// How far each corner of its box is rounded, in logical pixels.
    pub radius: f32,
    /// The font its text is laid out in.
    pub font: Font,
    /// The size of its text, in logical pixels to the em.
    pub font_size: f32,
    /// What it draws its content in: its text, a bar's fill, and the tint of an image.
    pub colour: Colour,
    /// Where each line of its text is set within its box.
    pub text_align: Align,
    /// A rim around each glyph of its text, or none: it draws outside the glyphs, so the text's box grows by its width.
    pub outline: Outline,
}

impl Default for Style {
    /// The initial value of every property, as CSS gives it where CSS has one: a box sized by its content, no space
    /// around it, a child that shrinks but does not grow, nothing drawn behind it, and white text in the renderer's
    /// own proportional font at sixteen pixels.
    fn default() -> Self {
        Self {
            width: Length::Auto,
            height: Length::Auto,
            min_width: Length::Auto,
            min_height: Length::Auto,
            max_width: Length::Auto,
            max_height: Length::Auto,
            margin: Edges::all(Length::Pixels(0.0)),
            padding: Edges::all(Space::Pixels(0.0)),
            gap: Gap { rows: Space::Pixels(0.0), columns: Space::Pixels(0.0) },
            grow: 0.0,
            shrink: 1.0,
            basis: Length::Auto,
            wrap: false,
            justify_content: Distribute::Normal,
            align_content: Distribute::Normal,
            align_items: Alignment::Normal,
            justify_items: Alignment::Normal,
            align_self: SelfAlign::Auto,
            justify_self: SelfAlign::Auto,
            columns: Vec::new(),
            rows: Vec::new(),
            column: Placed::Auto,
            row: Placed::Auto,
            background: Colour::CLEAR,
            border: Border { width: 0.0, colour: Colour::CLEAR },
            radius: 0.0,
            font: Font::Sans,
            font_size: 16.0,
            colour: Colour::WHITE,
            text_align: Align::Left,
            outline: Outline::None,
        }
    }
}

/// A size or a margin: worked out by layout, a length, or a share of the parent's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Auto,
    /// Logical pixels.
    Pixels(f32),
    /// A share of the parent's content box, from 0 to 1.
    Share(f32),
}

/// A padding or a gap: a length, or a share of the parent's content box's width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Space {
    /// Logical pixels.
    Pixels(f32),
    /// A share of the parent's content box's width, from 0 to 1.
    Share(f32),
}

/// One value for each side of a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edges<T> {
    pub left: T,
    pub right: T,
    pub top: T,
    pub bottom: T,
}

impl<T: Copy> Edges<T> {
    /// `value` on every side.
    pub const fn all(value: T) -> Self {
        Self { left: value, right: value, top: value, bottom: value }
    }
}

/// The space between a container's rows of children, and between its columns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gap {
    pub rows: Space,
    pub columns: Space,
}

/// How a container shares out the space its children leave along an axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distribute {
    /// The layout's own default: packed at the start, and lines stretched across a flex container.
    Normal,
    Start,
    End,
    Center,
    Stretch,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

/// Where children are set within the space they are given along an axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    /// The layout's own default: stretched, where the child has no size of its own along the axis.
    Normal,
    Start,
    End,
    Center,
    Baseline,
    Stretch,
}

/// Where a child sets itself: as its container says, or its own way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfAlign {
    Auto,
    Own(Alignment),
}

/// One track of a grid: its columns' or its rows' sizes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Track {
    /// Logical pixels.
    Pixels(f32),
    /// A share of the grid's content box, from 0 to 1.
    Share(f32),
    /// A part of the space the other tracks leave, against the other tracks' parts.
    Fraction(f32),
    /// As wide as its content, within the space there is.
    Auto,
    /// As narrow as its content goes.
    MinContent,
    /// As wide as its content wants.
    MaxContent,
}

/// Which of its grid's columns or rows a child stands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placed {
    /// The next free cell, one track across.
    Auto,
    /// From the line `line`, counted from 1 at the start, or from -1 at the end, across `span` tracks.
    At { line: i16, span: u16 },
    /// The next free cells, `span` tracks across.
    Span(u16),
}

/// A line around a box, inside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Border {
    /// Logical pixels.
    pub width: f32,
    pub colour: Colour,
}
