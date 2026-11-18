//! Drag and drop (ADR-0056: Drag and drop is one path): a widget its view gives a [`DragSource`] is dragged by a press
//! and a move past [`DRAG_THRESHOLD`], carrying the [`Payload`] the view copied as it was built; a widget given a
//! [`DropTarget`] takes a payload of the kind the method its drop calls takes last. Where the release lands decides the
//! rest, as the window reports it in the release's `over` (ADR-0055):
//!
//! ```text
//! press on a source ──> move past the threshold ──> the drag begins: the pressed hit's cursor is the drag's
//! release ──> over the screen's own hit ──> the innermost target around it that takes the kind?
//!         │       └─> the source's drop_accepted, then the target's drop
//!         ├─> over another actor's entry ──> cast to Drops? ──> Dropped, mailed ──> its library finds its target
//!         │       └─> DropAnswered, mailed back once ──> its outcome?
//!         │               ├─> Accepted: one took it, its drop raised ──> the source's drop_accepted
//!         │               └─> Refused: none took it ──> the source's drop_outside
//!         └─> over nothing, or nothing takes it ──> the source's drop_outside
//! ```
//!
//! The source hears how every drop ended, wherever it landed: `drop_accepted` when a target took it, so it lets go of
//! what moved, and `drop_outside` when nothing did. A payload crosses actors, so it is a kind, carried erased with its
//! kind's id: one kind of mail, [`Dropped`], serves every payload of every program, a target compares the id with the
//! kind its widget takes, and the method its drop calls reads the payload back as that kind ([`dropped`]). Between
//! actors its bytes are a blob (ADR-0042), put by the source as it mails the drop and read by the target as it takes it
//! in, and the answer hands the same blob back.

#[cfg(test)]
pub mod tests;

use crate::input::{Erased, Event, Raised};
use crate::tree::KeyPath;
use aethereal_actor::{Bytes, Fault, Level, Turn};
use aethereal_data::{Bound, Budget, Kind, KindId, Plain, from_bytes, to_bytes};
use aethereal_math::Vec2;
use aethereal_window::{Cursor, CursorImage};
use core::any::Any;
use core::fmt::Debug;

/// How far the pointer moves with the main button held on a source before its drag begins, in logical pixels: a press
/// and release that moved no further is still a click, as a hand that shakes while it clicks moves a pixel or two.
pub const DRAG_THRESHOLD: f32 = 4.0;

/// The most bytes a payload may decode into: far past an id.
const MOST_PAYLOAD_BYTES: usize = 64 * 1024;

/// What a drag carries: a value of a kind, `Copy` and compared by value, as an event's arguments are, so an id rather
/// than a snapshot of what may change before the drop; held as its kind's id and its bytes, so one kind of mail carries
/// a payload of any kind to another actor, and a target tells a payload it takes from one it does not.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.ui.payload")]
pub struct Payload {
    kind: KindId,
    bytes: Vec<u8>,
}

impl Payload {
    /// `value`, as a drag carries it.
    #[must_use]
    pub fn of<T: Kind + Plain + Copy + PartialEq>(value: T) -> Self {
        // A payload encodes as any plain value does: only a count past four billion fails, which no id holds.
        let bytes = to_bytes(&value).unwrap_or_default();

        Self { kind: KindId::of::<T>(), bytes }
    }

    /// The id of its value's kind.
    #[must_use]
    pub const fn kind(&self) -> KindId {
        self.kind
    }

    /// Its value, when it is a `T`: none for a payload of another kind, even one whose bytes would read as a `T`.
    #[must_use]
    pub fn read<T: Kind + Plain>(&self) -> Option<T> {
        let other = self.kind != KindId::of::<T>();
        if other {
            return None;
        }

        from_bytes(&self.bytes, &mut Budget::new(MOST_PAYLOAD_BYTES)).ok()
    }

    /// It as mail carries it, its bytes put as a blob, `blob`.
    pub(crate) const fn carried(&self, blob: Bytes) -> Carried {
        Carried { kind: self.kind, bytes: blob }
    }

    /// Its bytes, which mail carries as a blob.
    pub(crate) fn bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }
}

/// A payload as mail carries it between actors: its kind's id, and its bytes as a blob (ADR-0042).
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.ui.carried")]
pub struct Carried {
    pub kind: KindId,
    pub bytes: Bytes,
}

impl Carried {
    /// The payload it carries, its blob's bytes read with `read`, as a turn reads them:
    /// `dropped.payload.payload(|blob| turn.read(blob))`.
    #[must_use]
    pub fn payload<'bytes>(&self, read: impl FnOnce(&Bytes) -> &'bytes [u8]) -> Payload {
        Payload { kind: self.kind, bytes: read(&self.bytes).to_vec() }
    }
}

/// An event a node holds without its type, as the view copied it: compared by value with another, and raised with a
/// payload.
pub trait Copied: Debug + Send {
    /// It, to compare with another of the same type.
    fn as_any(&self) -> &dyn Any;
    /// Whether `other` is an event of the same type and the same value.
    fn same(&self, other: &dyn Copied) -> bool;
    /// It raised, with `payload` handed with it.
    fn raised(&self, payload: Payload) -> Raised<Erased>;
}

impl<E: Event> Copied for E {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Copied) -> bool {
        other.as_any().downcast_ref::<Self>().is_some_and(|other| self == other)
    }

    fn raised(&self, payload: Payload) -> Raised<Erased> {
        Raised::Dropped(*self, payload).erase()
    }
}

/// What the markup's `drag` reads its payload through: a reference may name a field that holds the payload,
/// `drag: item.id`, or a list's binding may be the payload itself, `items: self.ids as id key id.0 … drag: id`, which
/// `items` lends; a method call copies it out of either, reading through the loan.
#[doc(hidden)]
pub trait Lent: Kind + Plain + Copy + PartialEq {
    /// It, copied.
    #[must_use]
    fn lent(&self) -> Self {
        *self
    }
}

impl<T: Kind + Plain + Copy + PartialEq> Lent for T {}

/// What the markup's `drag`, `drop_accepted`, and `drop_outside` compile into: a widget a press and a move drag,
/// carrying `payload`, copied as the view was built; the cursor the window shows while it drags, its image or a closed
/// hand; and the events it raises with the payload when a target takes it, its `drop_accepted`, and when nothing takes
/// it, its `drop_outside`, each when it has one.
#[derive(Debug)]
pub struct DragSource {
    payload: Payload,
    cursor: Cursor,
    /// Its `drop_accepted`, fixed when the view builds it: none for a source the markup gives none, whose payload a
    /// target takes raises nothing at the source.
    accepted: Option<Box<dyn Copied>>,
    /// Its `drop_outside`, fixed when the view builds it: none for a source the markup gives none, whose payload
    /// dropped where nothing takes it raises nothing.
    outside: Option<Box<dyn Copied>>,
}

impl DragSource {
    /// A source that drags `payload`, showing a closed hand while it does, with no `drop_accepted` and no
    /// `drop_outside`.
    #[must_use]
    pub fn new<T: Kind + Plain + Copy + PartialEq>(payload: T) -> Self {
        Self { payload: Payload::of(payload), cursor: Cursor::Grabbing, accepted: None, outside: None }
    }

    /// It, raising `accepted` with its payload when a target takes it, in its own screen or another actor's.
    #[must_use]
    pub fn accepted<E: Event>(self, accepted: E) -> Self {
        Self { accepted: Some(Box::new(accepted)), ..self }
    }

    /// It, raising `outside` with its payload when nothing under the release takes it.
    #[must_use]
    pub fn outside<E: Event>(self, outside: E) -> Self {
        Self { outside: Some(Box::new(outside)), ..self }
    }

    /// It, showing `image` as the window's cursor while it drags, in place of the closed hand: the item dragged, drawn
    /// by the program as a cursor's image, at most [`MOST_CURSOR_SIDE`](aethereal_window::MOST_CURSOR_SIDE) on a side.
    #[must_use]
    pub fn image(self, image: CursorImage) -> Self {
        Self { cursor: Cursor::Image(image), ..self }
    }

    /// What it drags.
    pub(crate) const fn payload(&self) -> &Payload {
        &self.payload
    }

    /// The cursor the window shows while it drags.
    pub(crate) const fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    /// Its `drop_accepted` raised with `payload`, when it has one.
    pub(crate) fn accepted_raised(&self, payload: Payload) -> Option<Raised<Erased>> {
        self.accepted.as_ref().map(|accepted| accepted.raised(payload))
    }

    /// Its `drop_outside` raised with `payload`, when it has one.
    pub(crate) fn outside_raised(&self, payload: Payload) -> Option<Raised<Erased>> {
        self.outside.as_ref().map(|outside| outside.raised(payload))
    }
}

impl PartialEq for DragSource {
    fn eq(&self, other: &Self) -> bool {
        let payload = self.payload == other.payload;
        let cursor = self.cursor == other.cursor;
        let accepted = same_event(self.accepted.as_deref(), other.accepted.as_deref());
        let outside = same_event(self.outside.as_deref(), other.outside.as_deref());

        payload && cursor && accepted && outside
    }
}

/// Whether two events a source may raise are the same: both absent, or both the same value.
fn same_event(mine: Option<&dyn Copied>, theirs: Option<&dyn Copied>) -> bool {
    match (mine, theirs) {
        (Some(mine), Some(theirs)) => mine.same(theirs),
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
    }
}

/// What the markup's `drop` compiles into: a widget that takes a payload of the kind `takes`, raising `event`, copied as
/// the view was built, with the payload handed after it.
///
/// A bag whose slots each drag their item and take one dropped on them, in markup:
///
/// ```
/// use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Top, Turn};
/// use aethereal_data::{Bound, Plain};
/// use aethereal_render::frame::Frame;
/// use aethereal_render::pass::Interface;
/// use aethereal_ui::prelude::*;
/// # use aethereal_render::Renderer;
/// # use aethereal_render::frame::ShapeOf;
///
/// /// What a slot holds, and a drag from it carries.
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
/// #[kind(name = "aether.example.item_id")]
/// pub struct ItemId(u32);
///
/// /// A slot of the bag, at its place, and the item it holds.
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
/// #[kind(name = "aether.example.slot")]
/// pub struct Slot {
///     at: u32,
///     item: ItemId,
/// }
///
/// #[derive(Bound)]
/// #[kind(name = "aether.example.bag")]
/// pub struct Bag {
///     slots: Vec<Slot>,
///     dropped: Vec<ItemId>,
///     ui: Screen,
/// }
///
/// aethereal_ui::view! { Bag, ui,
///     Row { items: self.slots as slot key slot.at,
///         Stack { drag: slot.item, drop => self.place(slot.at), drop_outside => self.discard() }
///     }
/// }
///
/// impl Controls for Bag {
/// }
///
/// #[aethereal_ui::events]
/// impl Bag {
///     /// Puts `item` in the slot at `at`, and what that slot held where `item` was: the payload comes last.
///     fn place(&mut self, _: &mut Turn<'_, Self>, at: u32, item: ItemId) -> Result<(), Fault> {
///         let from = self.slots.iter().position(|slot| slot.item == item);
///         let to = self.slots.iter().position(|slot| slot.at == at);
///         if let (Some(from), Some(to)) = (from, to) {
///             self.slots[from].item = self.slots[to].item;
///             self.slots[to].item = item;
///         }
///
///         Ok(())
///     }
///
///     /// Drops `item` out of the bag, where nothing took it.
///     fn discard(&mut self, _: &mut Turn<'_, Self>, item: ItemId) -> Result<(), Fault> {
///         self.slots.retain(|slot| slot.item != item);
///         self.dropped.push(item);
///
///         Ok(())
///     }
/// }
/// #
/// # impl ChildOf for Bag {
/// #     type Parent = Top;
/// # }
/// #
/// # impl Actor for Bag {
/// #     type Config = Frame;
/// #     type Key = ();
/// #     type Handles = aethereal_ui::handles![];
/// #     type Depends = aethereal_ui::depends!();
/// #     type Spawns = ();
/// #
/// #     fn begin(frame: Frame, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
/// #         let ui = Screen::begin(Target::of::<Interface>(frame), Viewport::window(1, 1, 1.0), turn)?;
/// #         let slots = vec![Slot { at: 0, item: ItemId(1) }, Slot { at: 1, item: ItemId(2) }];
/// #         Ok(Self { slots, dropped: Vec::new(), ui })
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// ```
///
/// Each `Stack` it expands to is `Element::node(Stack {}, Segment::Item(ItemKey::from(slot.at)), vec![])`, then
/// `.with_drag(DragSource::new(slot.item).outside(BagEvent::Discard))` and
/// `.with_drop(DropTarget::new(BagEvent::Place(slot.at), Self::place))`.
#[derive(Debug)]
pub struct DropTarget {
    takes: KindId,
    event: Box<dyn Copied>,
}

impl DropTarget {
    /// A target that raises `event` and takes a payload of the kind `method` takes last: `method` is the screen's
    /// method the event calls, named as a path, `Self::place`, so the kind is the one the method is written to take, and
    /// a payload of any other is refused. Nothing calls `method` here.
    #[must_use]
    pub fn new<E: Event, M: DropMethod<Marker> + Copy, Marker>(event: E, method: M) -> Self
    where
        M::Payload: Kind,
    {
        let _ = method;

        Self { takes: KindId::of::<M::Payload>(), event: Box::new(event) }
    }

    /// Whether it takes a payload of the kind `kind`.
    pub(crate) fn takes(&self, kind: KindId) -> bool {
        self.takes == kind
    }

    /// Its event raised with `payload`.
    pub(crate) fn raised(&self, payload: Payload) -> Raised<Erased> {
        self.event.raised(payload)
    }
}

impl PartialEq for DropTarget {
    fn eq(&self, other: &Self) -> bool {
        let takes = self.takes == other.takes;
        let event = self.event.same(&*other.event);

        takes && event
    }
}

/// A function whose last parameter is `Payload`: the method a drop calls, whatever comes before it, the screen, its
/// turn, and the arguments the view copied. `Marker` is the function's signature, which the compiler infers.
pub trait DropMethod<Marker> {
    /// The kind of its last parameter.
    type Payload;
}

/// A function of each count of parameters names its last.
macro_rules! drop_methods {
    ($( [$($before:ident)*] )*) => {$(
        impl<F, $($before,)* Last, Out> DropMethod<fn($($before,)* Last) -> Out> for F
        where
            F: FnOnce($($before,)* Last) -> Out,
        {
            type Payload = Last;
        }
    )*};
}

drop_methods! {
    []
    [A]
    [A B]
    [A B C]
    [A B C D]
    [A B C D G]
    [A B C D G H]
    [A B C D G H I]
    [A B C D G H I J]
}

/// What a drag dropped on another actor's entry mails it: the payload; the entry of its list the release was over, and
/// the release's place in that entry's coordinates, as the window reported them; and the widget it was dragged from, in
/// its sender's tree, which the answer hands back. A screen's library finds the target at the entry, raises its drop,
/// and answers whether it took it; an actor that is not a screen handles it as it will, and answers too, once:
/// [`Dropped::answer_source`] with [`DropOutcome::Accepted`] to have the source raise its `drop_accepted`, and with
/// [`DropOutcome::Refused`] its `drop_outside`.
#[derive(Debug, Clone, PartialEq, Bound)]
#[kind(name = "aether.ui.dropped")]
pub struct Dropped {
    pub payload: Carried,
    pub entry: u32,
    pub at: Vec2,
    pub source: KeyPath,
}

impl Dropped {
    /// The answer to it: its payload and its source, handed back, with `outcome`, on which the source raises its
    /// `drop_accepted` or its `drop_outside`.
    #[must_use]
    pub fn answer(self, outcome: DropOutcome) -> DropAnswered {
        DropAnswered { payload: self.payload, source: self.source, outcome }
    }

    /// Mails its [`answer`](Self::answer) with `outcome` to the actor that dropped it, in the turn that handles it: a
    /// drop is mailed as a tell to an actor cast to [`Drops`], so its source hears the answer as mail of its own, and
    /// one that does not handle [`DropAnswered`] is told nothing, which is logged.
    ///
    /// # Errors
    ///
    /// Why the answer could not be written.
    pub fn answer_source<A>(self, turn: &mut Turn<'_, A>, outcome: DropOutcome) -> Result<(), Fault> {
        let sender = turn.sender();
        let Some(source) = turn.cast::<(DropAnswered,)>(sender) else {
            turn.log(Level::Warn, "a drop came from an actor that does not handle its answer");
            return Ok(());
        };
        turn.to(source).send(&self.answer(outcome))?;

        Ok(())
    }
}

/// What an actor a drag was dropped on answers, once for each [`Dropped`]: the payload and the widget it was dragged
/// from, handed back, and whether it took the payload, so the source lets go of what moved or keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.ui.drop_answered")]
pub struct DropAnswered {
    pub payload: Carried,
    pub source: KeyPath,
    pub outcome: DropOutcome,
}

/// How a drop on another actor ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum DropOutcome {
    /// A target there took the payload: the source raises its `drop_accepted`.
    Accepted,
    /// Nothing there took it: the source raises its `drop_outside`.
    Refused,
}

/// What an actor a drag can be dropped on takes: every screen, as [`handles!`](crate::handles!) lists it, and any other
/// actor that handles [`Dropped`]. A source casts the actor a release was over to it, and an actor it does not reach
/// took nothing.
pub type Drops = (Dropped,);

/// Calls `method` with `turn` and `payload` read as the kind `method` takes: what a screen's generated
/// [`View::on`](crate::View::on) calls for a drop, a `drop_accepted`, and a `drop_outside`, the payload appended after
/// the arguments the view copied. A payload of another kind, which only a tree built by hand can raise, is logged.
///
/// # Errors
///
/// What `method` returns.
pub fn dropped<A, T: Kind + Plain>(
    turn: &mut Turn<'_, A>,
    payload: &Payload,
    method: impl FnOnce(&mut Turn<'_, A>, T) -> Result<(), Fault>,
) -> Result<(), Fault> {
    let Some(value) = payload.read::<T>() else {
        turn.log(Level::Error, format!("a payload came of a kind its method does not take: {payload:?}"));
        return Ok(());
    };

    method(turn, value)
}
