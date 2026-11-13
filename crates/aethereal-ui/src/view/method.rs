//! The call of a screen's method an event names (ADR-0056: Events call the screen's methods): with the turn, the
//! arguments the view copied, and, when the method takes one, what the widget handed with the event.
//!
//! ```text
//! Raised::Event(Select(item))         ──> select(turn, item)               a button's click hands nothing
//! Raised::Text(Rename(item), text)    ──> rename(turn, item, text: String) a text input's change hands its text
//! Raised::Flag(Show(item), on)        ──> show(turn, item, on: bool)       a toggle's change, whether it is on
//! Raised::Number(Volume, value)       ──> volume(turn, value: f32)         a slider's change, its value
//! Raised::Index(Open, tab)            ──> open(turn, tab: u32)             tabs' change, the tab chosen
//! ```
//!
//! A screen's markup names a widget's event and the method it calls, but not what the widget hands, which only the
//! widget's type knows. The method's signature says it: rustc picks the call by the parameters the method has, as a
//! drop's [`DropMethod`](crate::DropMethod) is read, so a method that takes a hand gets it, by its type. An event raised
//! with another hand than its method takes, which only a tree built by hand can raise, is logged as
//! [`unmatched`](super::unmatched).

use super::{View, unmatched};
use crate::input::Raised;
use aethereal_actor::{Fault, Turn};

/// What a widget hands with its event, read from what it raised.
pub trait Handed: Sized {
    /// The value `raised` hands, or `raised` as it was when it hands none of this type.
    ///
    /// # Errors
    ///
    /// `raised`, when it hands something else or nothing.
    fn handed<E>(raised: Raised<E>) -> Result<Self, Raised<E>>;
}

/// A text input's text.
impl Handed for String {
    fn handed<E>(raised: Raised<E>) -> Result<Self, Raised<E>> {
        match raised {
            Raised::Text(_, text) => Ok(text),
            other => Err(other),
        }
    }
}

/// A toggle's flag.
impl Handed for bool {
    fn handed<E>(raised: Raised<E>) -> Result<Self, Raised<E>> {
        match raised {
            Raised::Flag(_, on) => Ok(on),
            other => Err(other),
        }
    }
}

/// A slider's value.
impl Handed for f32 {
    fn handed<E>(raised: Raised<E>) -> Result<Self, Raised<E>> {
        match raised {
            Raised::Number(_, value) => Ok(value),
            other => Err(other),
        }
    }
}

/// Tabs' choice, counted from zero.
impl Handed for u32 {
    fn handed<E>(raised: Raised<E>) -> Result<Self, Raised<E>> {
        match raised {
            Raised::Index(_, index) => Ok(index),
            other => Err(other),
        }
    }
}

/// A screen's method an event calls: a function of the screen, its turn, the arguments `Args` the view copied, and
/// what the widget hands when it takes it. `Marker` is the method's signature, which the compiler infers.
pub trait Method<A: View, Args, Marker> {
    /// Calls it with `args`, and with what `raised` hands when it takes a hand.
    ///
    /// # Errors
    ///
    /// The method's fault.
    fn call(
        self,
        screen: &mut A,
        turn: &mut Turn<'_, A, A::Sends>,
        args: Args,
        raised: Raised<A::Event>,
    ) -> Result<(), Fault>;
}

/// A method of each count of copied arguments, taking a hand after them or none.
macro_rules! methods {
    ($( [$($argument:ident $value:ident)*] )*) => {$(
        impl<A: View, F, $($argument,)*> Method<A, ($($argument,)*), fn($($argument,)*)> for F
        where
            F: FnOnce(&mut A, &mut Turn<'_, A, A::Sends>, $($argument,)*) -> Result<(), Fault>,
        {
            fn call(
                self,
                screen: &mut A,
                turn: &mut Turn<'_, A, A::Sends>,
                ($($value,)*): ($($argument,)*),
                raised: Raised<A::Event>,
            ) -> Result<(), Fault> {
                match raised {
                    Raised::Event(_) => self(screen, turn, $($value,)*),
                    other => unmatched(turn, &other),
                }
            }
        }

        impl<A: View, F, $($argument,)* H: Handed> Method<A, ($($argument,)*), fn($($argument,)* H)> for F
        where
            F: FnOnce(&mut A, &mut Turn<'_, A, A::Sends>, $($argument,)* H) -> Result<(), Fault>,
        {
            fn call(
                self,
                screen: &mut A,
                turn: &mut Turn<'_, A, A::Sends>,
                ($($value,)*): ($($argument,)*),
                raised: Raised<A::Event>,
            ) -> Result<(), Fault> {
                match H::handed(raised) {
                    Ok(hand) => self(screen, turn, $($value,)* hand),
                    Err(other) => unmatched(turn, &other),
                }
            }
        }
    )*};
}

methods! {
    []
    [B0 b0]
    [B0 b0 B1 b1]
    [B0 b0 B1 b1 B2 b2]
    [B0 b0 B1 b1 B2 b2 B3 b3]
    [B0 b0 B1 b1 B2 b2 B3 b3 B4 b4]
    [B0 b0 B1 b1 B2 b2 B3 b3 B4 b4 B5 b5]
}
