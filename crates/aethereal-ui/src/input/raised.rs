//! An event a widget raises: a value of its screen's event type, and the value the widget hands with it, which the
//! screen's method takes after the arguments the view copied.
//!
//! A tree holds widgets of whatever type, so what they raise is held without its screen's event type, [`Erased`], and
//! turned back into it as the screen takes it ([`Raised::typed`]). The markup builds every event of a screen's widgets
//! from the screen's own event type, so none is of another.

use core::any::Any;
use core::fmt::Debug;

/// A screen's event: what its markup's event attributes compile into, one variant for each, holding the arguments the
/// view copied as it was built. Those are `Copy`, ids rather than snapshots of what may change before the click, so an
/// event is too.
pub trait Event: Debug + PartialEq + Copy + Send + 'static {}

impl<E: Debug + PartialEq + Copy + Send + 'static> Event for E {}

/// An event raised, and the value its widget hands with it: none, as a button's click; the text, as a text input's
/// change and submit; whether it is on, as a toggle's change; or a number, as a slider's.
#[derive(Debug, Clone, PartialEq)]
pub enum Raised<E> {
    Event(E),
    Text(E, String),
    Flag(E, bool),
    Number(E, f32),
}

/// A screen's event, held without its type.
#[derive(Debug)]
pub struct Erased(Box<dyn Any + Send>);

impl<E: Event> Raised<E> {
    /// It, its event held without its type.
    pub(crate) fn erase(self) -> Raised<Erased> {
        let erased = |event: E| Erased(Box::new(event));
        match self {
            Self::Event(event) => Raised::Event(erased(event)),
            Self::Text(event, text) => Raised::Text(erased(event), text),
            Self::Flag(event, on) => Raised::Flag(erased(event), on),
            Self::Number(event, value) => Raised::Number(erased(event), value),
        }
    }
}

impl Raised<Erased> {
    /// It, its event of the type `E`.
    ///
    /// # Errors
    ///
    /// It, as it was, when its event is not an `E`: one a widget of another screen's events raised.
    pub fn typed<E: Event>(self) -> Result<Raised<E>, Self> {
        match self {
            Self::Event(event) => event.typed().map(Raised::Event).map_err(Self::Event),
            Self::Text(event, text) => match event.typed() {
                Ok(event) => Ok(Raised::Text(event, text)),
                Err(event) => Err(Self::Text(event, text)),
            },
            Self::Flag(event, on) => event.typed().map(|event| Raised::Flag(event, on)).map_err(|e| Self::Flag(e, on)),
            Self::Number(event, value) => {
                event.typed().map(|event| Raised::Number(event, value)).map_err(|e| Self::Number(e, value))
            }
        }
    }
}

impl Erased {
    /// The event, of the type `E`.
    fn typed<E: Event>(self) -> Result<E, Self> {
        match self.0.downcast::<E>() {
            Ok(event) => Ok(*event),
            Err(other) => Err(Self(other)),
        }
    }
}
