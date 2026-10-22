//! What roots provide the roots bootstrapped after them, found by type (ADR-0029).
//!
//! What the roots bootstrapped so far provided is a list, `()` for nothing and `(T, P)` for a `T` provided after what
//! `P` holds. A root's `Needs` is a tuple of types, each found in that list at a place the compiler infers, as
//! `Ordered` finds a root (ADR-0022), and handed to it as a clone. A root's `Provides` is a tuple too, put before the
//! list.

use aethereal_actor::{Here, There};

/// The list of what earlier roots provided holds a `T`, at the place `I`, which the compiler infers.
#[diagnostic::on_unimplemented(
    message = "no root listed before the root that needs `{T}` provides it",
    label = "a root here needs `{T}`",
    note = "list a root whose `Provides` holds `{T}` before the root whose `Needs` does"
)]
pub trait Provided<T, I> {
    /// The `T` it holds.
    fn provided(&self) -> &T;
}

impl<T, P> Provided<T, Here> for (T, P) {
    fn provided(&self) -> &T {
        &self.0
    }
}

impl<T, U, P: Provided<T, I>, I> Provided<T, There<I>> for (U, P) {
    fn provided(&self) -> &T {
        self.1.provided()
    }
}

/// The list of what earlier roots provided holds each type of the tuple `N`, at the places `I`.
pub trait Gathers<N, I> {
    /// A clone of each value `N` names.
    fn gather(&self) -> N;
}

impl<P> Gathers<(), ()> for P {
    fn gather(&self) {}
}

/// A tuple a root provides, put before what the roots bootstrapped earlier provided.
pub trait Provision<P> {
    /// The list with this tuple's values before `P`'s.
    type Onto;

    /// The list with these values before `earlier`.
    fn onto(self, earlier: P) -> Self::Onto;
}

impl<P> Provision<P> for () {
    type Onto = P;

    fn onto(self, earlier: P) -> P {
        earlier
    }
}

/// The list `$rest` with the types `$name` before it, in order.
macro_rules! listed {
    ($rest:ty;) => { $rest };
    ($rest:ty; $head:ident $($tail:ident)*) => { ($head, listed!($rest; $($tail)*)) };
}

/// The list `$rest` with the values bound as `$name` before it, in order.
macro_rules! listing {
    ($rest:expr;) => { $rest };
    ($rest:expr; $head:ident $($tail:ident)*) => { ($head, listing!($rest; $($tail)*)) };
}

macro_rules! tuples {
    ($( [$($name:ident $index:ident)+] )*) => {$(
        impl<P, $($name: Clone, $index),+> Gathers<($($name,)+), ($($index,)+)> for P
        where
            $(P: Provided<$name, $index>,)+
        {
            fn gather(&self) -> ($($name,)+) {
                ($(Provided::<$name, $index>::provided(self).clone(),)+)
            }
        }

        impl<P, $($name),+> Provision<P> for ($($name,)+) {
            type Onto = listed!(P; $($name)+);

            #[allow(non_snake_case, reason = "each value is bound under its type's name, which the list is built from")]
            fn onto(self, earlier: P) -> Self::Onto {
                let ($($name,)+) = self;
                listing!(earlier; $($name)+)
            }
        }
    )*};
}

tuples! {
    [A IA]
    [A IA B IB]
    [A IA B IB C IC]
    [A IA B IB C IC D ID]
    [A IA B IB C IC D ID E IE]
    [A IA B IB C IC D ID E IE F IF]
    [A IA B IB C IC D ID E IE F IF G IG]
    [A IA B IB C IC D ID E IE F IF G IG H IH]
}
