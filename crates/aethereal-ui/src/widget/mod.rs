//! What a widget is (ADR-0056: Your own widgets): a value of its settings, which takes part in layout as it arranges
//! its children, measures itself when it is a leaf, and draws itself into its screen's draw list. A primitive, stock
//! or a program's own, implements [`Widget`]; a composite is markup the build expands into primitives, so it implements
//! nothing here.
//!
//! A widget keeps no state of its own. What it needs between rebuilds, hover or a scroll offset, is its
//! [`Widget::State`], which the screen's widget table makes, keeps, and drops by the widget's key (ADR-0056: Widget
//! state is kept by key).
//!
//! Input reaches a primitive by the entry of its screen's list it is reached through: the pointer in its own
//! coordinates, and, while it has its screen's focus, keys and typed text. It changes its state, and raises its events
//! (ADR-0056: Events call the screen's methods). Each has a default, so a widget that takes no input says nothing of it.

mod draws;

pub use draws::{Draws, Quad, Sample, Slices, TextDraw};
pub use draws::{Focused, Piece};

use crate::fonts::Measuring;
use crate::input::{Hears, Input, Key, Pointing, Used};
use crate::style::{Edges, Style};
use aethereal_data::{Budget, Kind, KindId, Plain, from_bytes, to_bytes};
use aethereal_math::{Rect2, Vec2};
use aethereal_render::draw_list::Pointer;
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
/// It takes in input as its methods with defaults say: a widget the pointer reaches says so in [`Self::pointer`], and
/// one that can hold its screen's focus in [`Self::focusable`].
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

    /// The room it keeps on each side for what it draws of its own, in pixels: inside its border and outside its
    /// padding, so its children are laid out within it, as a scroll keeps room for its bar and tabs for their headers.
    fn room(&self, style: &Style, measuring: &mut Measuring<'_>) -> Edges<f32> {
        let _ = (style, measuring);

        Edges::all(0.0)
    }

    /// Which items of a list its children are, for a widget that arranges them as a scroll: every child it scrolls
    /// through, or the window a list built of its items.
    fn lines(&self) -> &Lines {
        &WHOLE
    }

    /// How far it has slid its children along its axis, in pixels, as `state` stands: for a widget that arranges
    /// them as a scroll.
    fn offset(&self, state: &Self::State) -> f32 {
        let _ = state;

        0.0
    }

    /// Slides its children to `offset` pixels along its axis, kept in `state`: the library calls it with an offset
    /// within what can be scrolled, as layout clamps it, the wheel turns it, and focus moved by Tab is shown.
    fn scroll_to(&self, state: &mut Self::State, offset: f32) {
        let _ = (state, offset);
    }

    /// Draws its content where layout put it, `laid`, over the background and border its style draws beneath it.
    fn draw(&self, state: &Self::State, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>);

    /// How the pointer reaches it over its box, as `state` stands, and the cursor the window shows there: a widget the
    /// pointer reaches is listed with a hit over its box, after its draws and before its children's. One it ignores is
    /// reached only when its style fills its box, as a panel's background blocks the pointer from what is beneath it.
    fn pointer(&self, state: &Self::State) -> Pointer {
        let _ = state;

        Pointer::Ignores
    }

    /// Whether it can hold its screen's focus: a press on it focuses it, Tab reaches it in tree order, and keys go to
    /// it first while it has it.
    fn focusable(&self) -> bool {
        false
    }

    /// What it hears while it has its screen's focus.
    fn hears(&self) -> Hears {
        Hears::Keys(&[])
    }

    /// Takes in what the pointer did at it.
    fn pointed(&self, state: &mut Self::State, pointing: Pointing, input: &mut Input<'_, '_>) {
        let _ = (state, pointing, input);
    }

    /// Takes in a key pressed while it has its screen's focus, and says whether it used it.
    fn key(&self, state: &mut Self::State, key: &Key, input: &mut Input<'_, '_>) -> Used {
        let _ = (state, key, input);

        Used::Passed
    }

    /// Takes in text typed while it has its screen's focus: the characters a key typed, or what an input method
    /// committed (ADR-0051).
    fn typed(&self, state: &mut Self::State, text: &str, input: &mut Input<'_, '_>) {
        let _ = (state, text, input);
    }
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
    /// One after another along an axis, as `Flex` but never shrunk to fit, cut to its content box, and slid along it by
    /// its offset: placed where its [`Lines`] say when it built only some of a list.
    Scroll(Axis),
    /// It has no children, and measures its own content.
    Leaf,
}

/// The axis a flex container runs along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Row,
    Column,
}

impl Axis {
    /// How far `vector` runs along it.
    #[must_use]
    pub const fn along(self, vector: Vec2) -> f32 {
        match self {
            Self::Row => vector.x,
            Self::Column => vector.y,
        }
    }

    /// A vector `length` along it.
    pub const fn vector(self, length: f32) -> Vec2 {
        match self {
            Self::Row => Vec2::new(length, 0.0),
            Self::Column => Vec2::new(0.0, length),
        }
    }
}

/// Which items of a list a scroll's children are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lines {
    /// Its children are all it scrolls through, laid out one after another.
    Whole,
    /// Its children are the items of a list of `count` at the places `at`, in order: one line each, the lines of the
    /// list each as long as the first built, so the items it did not build still take their room.
    Window { count: u32, at: Vec<u32> },
}

/// The lines of every widget that builds all its children.
static WHOLE: Lines = Lines::Whole;

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

/// Where layout put a widget, in its screen's pixels, from the screen's top left: its box, the box inside its border,
/// room, and padding, which its content fills, what it is cut to, and how far its children reach.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Laid {
    /// Its border box.
    pub rect: Rect2,
    /// Its content box: the border box less its border, its room, and its padding.
    pub content: Rect2,
    /// What its drawing is cut to: its screen's box, within every ancestor that cuts its children.
    pub clip: Rect2,
    /// How far its children reach from its content box's top left, across and down, before it slides them: what a
    /// scroll scrolls through, the items of its list it did not build counted.
    pub extent: Vec2,
    /// How many pixels a logical pixel is.
    pub scale: f32,
}

impl Laid {
    /// It, in coordinates whose `0, 0` is at `origin` of these.
    #[must_use]
    pub fn relative_to(self, origin: Vec2) -> Self {
        let moved = |rect: Rect2| Rect2 { min: rect.min - origin, max: rect.max - origin };

        Self { rect: moved(self.rect), content: moved(self.content), clip: moved(self.clip), ..self }
    }

    /// How far its children can slide along `axis`: how far they reach past its content box, or none.
    #[must_use]
    pub fn travel(&self, axis: Axis) -> f32 {
        (axis.along(self.extent) - axis.along(self.content.size())).max(0.0)
    }
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
    /// The room it keeps for its own drawing.
    fn room(&self, style: &Style, measuring: &mut Measuring<'_>) -> Edges<f32>;
    /// Which items of a list its children are.
    fn lines(&self) -> &Lines;
    /// How far it has slid its children, its state read from `state`.
    fn offset(&self, state: &[u8]) -> f32;
    /// Slides its children to `offset`, its state read from and written back to `state`.
    fn scroll_to(&self, state: &mut Vec<u8>, offset: f32);
    /// The kind of its state, which tells a state of its type from one of another's.
    fn state_kind(&self) -> KindId;
    /// Its state as it is first made, encoded.
    fn first_state(&self) -> Vec<u8>;
    /// Draws it with its state read from `state`, the bytes its table keeps: a state that does not read, which only a
    /// table not kept by these rules could hold, is drawn as it is first made.
    fn draw(&self, state: &[u8], style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>);
    /// How the pointer reaches it, its state read from `state`.
    fn pointer(&self, state: &[u8]) -> Pointer;
    /// Whether it can hold its screen's focus.
    fn focusable(&self) -> bool;
    /// What it hears while it has its screen's focus.
    fn hears(&self) -> Hears;
    /// Takes in what the pointer did at it, its state read from and written back to `state`.
    fn pointed(&self, state: &mut Vec<u8>, pointing: Pointing, input: &mut Input<'_, '_>);
    /// Takes in a key, its state read from and written back to `state`, and says whether it used it.
    fn key(&self, state: &mut Vec<u8>, key: &Key, input: &mut Input<'_, '_>) -> Used;
    /// Takes in typed text, its state read from and written back to `state`.
    fn typed(&self, state: &mut Vec<u8>, text: &str, input: &mut Input<'_, '_>);
}

/// `W`'s state read from `state`, the bytes its table keeps: one that does not read, which only a table not kept by
/// these rules could hold, is read as it is first made.
fn read<W: Widget>(state: &[u8]) -> W::State {
    from_bytes::<W::State>(state, &mut Budget::new(MOST_STATE_BYTES)).unwrap_or_default()
}

/// Reads `W`'s state from `state`, changes it with `change`, and writes it back, answering what `change` did.
fn changed<W: Widget, T>(state: &mut Vec<u8>, change: impl FnOnce(&mut W::State) -> T) -> T {
    let mut read = read::<W>(state);
    let did = change(&mut read);
    // A state encodes as any plain value does: only a count past four billion fails, which no widget's state holds.
    if let Ok(bytes) = to_bytes(&read) {
        *state = bytes;
    }

    did
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

    fn room(&self, style: &Style, measuring: &mut Measuring<'_>) -> Edges<f32> {
        Widget::room(self, style, measuring)
    }

    fn lines(&self) -> &Lines {
        Widget::lines(self)
    }

    fn offset(&self, state: &[u8]) -> f32 {
        Widget::offset(self, &read::<W>(state))
    }

    fn scroll_to(&self, state: &mut Vec<u8>, offset: f32) {
        changed::<W, ()>(state, |state| Widget::scroll_to(self, state, offset));
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
        Widget::draw(self, &read::<W>(state), style, laid, draws);
    }

    fn pointer(&self, state: &[u8]) -> Pointer {
        Widget::pointer(self, &read::<W>(state))
    }

    fn focusable(&self) -> bool {
        Widget::focusable(self)
    }

    fn hears(&self) -> Hears {
        Widget::hears(self)
    }

    fn pointed(&self, state: &mut Vec<u8>, pointing: Pointing, input: &mut Input<'_, '_>) {
        changed::<W, ()>(state, |state| Widget::pointed(self, state, pointing, input));
    }

    fn key(&self, state: &mut Vec<u8>, key: &Key, input: &mut Input<'_, '_>) -> Used {
        changed::<W, Used>(state, |state| Widget::key(self, state, key, input))
    }

    fn typed(&self, state: &mut Vec<u8>, text: &str, input: &mut Input<'_, '_>) {
        changed::<W, ()>(state, |state| Widget::typed(self, state, text, input));
    }
}
