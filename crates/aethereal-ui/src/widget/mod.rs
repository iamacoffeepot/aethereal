//! What a widget is (ADR-0056: Your own widgets): a value of its settings, which takes part in layout as it arranges
//! its children, measures itself when it is a leaf, and draws itself into its screen's draw list. A primitive, stock
//! or a program's own, implements [`Widget`]; a composite is markup the build expands into primitives, so it implements
//! nothing here.
//!
//! A widget keeps no state of its own. What it needs between rebuilds, hover or a scroll offset, is its
//! [`Widget::State`], which the screen's widget table makes, keeps, and drops by the widget's key (ADR-0056: Widget
//! state is kept by key).

mod draws;

pub use draws::Piece;
pub use draws::{Draws, Quad, Sample, Slices, TextDraw};

use crate::fonts::Measuring;
use crate::style::Style;
use aethereal_data::{Budget, Kind, KindId, Plain, from_bytes, to_bytes};
use aethereal_math::{Rect2, Vec2};
use core::any::Any;
use core::fmt::Debug;

/// The most bytes a widget's state may decode into: far past a widget's hover, scroll offset, or caret.
const MOST_STATE_BYTES: usize = 64 * 1024;

/// A primitive widget: its settings, as the view builds them each `Prepare`, compared with the last by value.
///
/// It says how it arranges its children, measures itself when it is a leaf, and draws itself where layout put it. Its
/// state is [`Self::State`], made with `Default` the first time its key appears, kept across rebuilds and moves onto new
/// code, and dropped when its key goes: a kind, so a state whose type changed, at the same key or in new code, is made
/// anew rather than misread.
///
/// Input is not part of it yet. The library's input stage adds it as methods with defaults, so a widget written now
/// keeps building.
pub trait Widget: Debug + PartialEq + Send + 'static {
    /// What the widget keeps between rebuilds: [`Stateless`] for a widget that keeps nothing.
    type State: Kind + Plain + Default;

    /// How it lays its children out.
    fn arrange(&self) -> Arrange;

    /// The size of its content, in pixels, within `offer`: called for a leaf alone, as layout asks. A leaf with no
    /// content of its own, sized by its style, measures nothing.
    fn measure(&self, style: &Style, measuring: &mut Measuring<'_>, offer: Offer) -> Vec2 {
        let _ = (style, measuring, offer);

        Vec2::ZERO
    }

    /// Draws its content where layout put it, `laid`, over the background and border its style draws beneath it.
    fn draw(&self, state: &Self::State, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>);
}

/// How a widget lays its children out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrange {
    /// One after another along an axis, by its style's flex settings.
    Flex(Axis),
    /// In the cells of its style's columns and rows.
    Grid,
    /// Each over the one before, in one cell its size.
    Stack,
    /// It has no children, and measures its own content.
    Leaf,
}

/// The axis a flex container runs along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Row,
    Column,
}

/// The space a leaf is offered along each axis, as layout asks it to measure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Offer {
    pub width: Offered,
    pub height: Offered,
}

/// The space offered along one axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Offered {
    /// Exactly this many pixels: layout has fixed the size.
    Exactly(f32),
    /// Up to this many pixels.
    AtMost(f32),
    /// As little as the content goes: a text's widest word.
    MinContent,
    /// As much as the content wants: a text on as few lines as its breaks allow.
    MaxContent,
}

/// Where layout put a widget, in its screen's pixels, from the screen's top left: its box, the box inside its border
/// and padding, which its content fills, and what it is cut to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Laid {
    /// Its border box.
    pub rect: Rect2,
    /// Its content box: the border box less its border and padding.
    pub content: Rect2,
    /// What its drawing is cut to: its screen's box, within every ancestor that cuts its children.
    pub clip: Rect2,
    /// How many pixels a logical pixel is.
    pub scale: f32,
}

/// The state of a widget that keeps nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
#[kind(name = "aether.ui.stateless")]
pub struct Stateless;

/// A widget as a tree holds it, of whatever type: what the library asks of it without naming it.
pub trait Built: Debug + Send {
    /// It, to compare with another of the same type.
    fn as_any(&self) -> &dyn Any;
    /// Whether `other` is a widget of the same type and the same settings.
    fn same(&self, other: &dyn Built) -> bool;
    /// How it lays its children out.
    fn arrange(&self) -> Arrange;
    /// The size of its content within `offer`.
    fn measure(&self, style: &Style, measuring: &mut Measuring<'_>, offer: Offer) -> Vec2;
    /// The kind of its state, which tells a state of its type from one of another's.
    fn state_kind(&self) -> KindId;
    /// Its state as it is first made, encoded.
    fn first_state(&self) -> Vec<u8>;
    /// Draws it with its state read from `state`, the bytes its table keeps: a state that does not read, which only a
    /// table not kept by these rules could hold, is drawn as it is first made.
    fn draw(&self, state: &[u8], style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>);
}

impl<W: Widget> Built for W {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Built) -> bool {
        other.as_any().downcast_ref::<Self>().is_some_and(|other| self == other)
    }

    fn arrange(&self) -> Arrange {
        Widget::arrange(self)
    }

    fn measure(&self, style: &Style, measuring: &mut Measuring<'_>, offer: Offer) -> Vec2 {
        Widget::measure(self, style, measuring, offer)
    }

    fn state_kind(&self) -> KindId {
        KindId::of::<W::State>()
    }

    fn first_state(&self) -> Vec<u8> {
        // A state's default encodes as any plain value does: only a count past four billion fails, which no state's
        // default holds.
        to_bytes(&W::State::default()).unwrap_or_default()
    }

    fn draw(&self, state: &[u8], style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let state = from_bytes::<W::State>(state, &mut Budget::new(MOST_STATE_BYTES)).unwrap_or_default();
        Widget::draw(self, &state, style, laid, draws);
    }
}
