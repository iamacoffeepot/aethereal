//! An event a widget may raise or not, and the type a widget raises its events as: what lets markup write every event
//! the same way, `name => call`, whether the widget must raise it or may.
//!
//! ```text
//! TextInput { change => self.search(), submit => self.send() }
//!     ──> raising::<ShopEvent, _>(TextInput { change: Into::into(ShopEvent::Search), submit: Into::into(ShopEvent::Send), … })
//!             change: E           ShopEvent ──Into──> ShopEvent                 an event it must raise
//!             submit: Raises<E>   ShopEvent ──Into──> Raises::Event(ShopEvent)  an event it may raise
//! ```
//!
//! A widget generic over its event type, `Button<E>`, is told it only by the events its markup writes, and `Into` reaches
//! more than one type from the screen's event, so the markup also names the type the widget raises as, the screen's
//! event type or a composite's `E`, through [`Raising`].

use super::Event;

/// An event a widget may raise, set where markup writes it and raising nothing where markup leaves it out: a text
/// input's `submit`. An event a widget must raise is its event type itself, `E`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Raises<E> {
    /// It raises nothing.
    Nothing,
    /// It raises this event.
    Event(E),
}

/// What markup's `..` leaves an optional event at: it raises nothing.
impl<E> Default for Raises<E> {
    fn default() -> Self {
        Self::Nothing
    }
}

/// An event markup writes with `=>`, as an optional event's field takes it.
impl<E> From<E> for Raises<E> {
    fn from(event: E) -> Self {
        Self::Event(event)
    }
}

/// A widget that raises events, by the type it raises them as: a stock widget's `E`, as in `Button<E>`, or a
/// composite's, which `widget!` declares. Markup sets each event of an element through `Into`, so an event it must
/// raise, `E`, and one it may, [`Raises<E>`], are written alike; the type named here is the one both convert to.
#[diagnostic::on_unimplemented(
    message = "`{Self}` raises no events",
    label = "an event written with `=>` is a field of a widget that implements `Raising`"
)]
pub trait Raising {
    /// The type of the events it raises: its screen's event type.
    type Event: Event;
}

/// `widget`, raising its events as `E`: what markup writes around an element that sets an event, `E` the screen's
/// event type, or a composite's own, so the events it converts into the element's fields convert to `E`.
#[must_use]
pub const fn raising<E: Event, W: Raising<Event = E>>(widget: W) -> W {
    widget
}
