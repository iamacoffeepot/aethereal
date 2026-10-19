//! What a reference typed by a protocol is proven to reach: a type that conforms to it, and a protocol that covers
//! another (ADR-0016).

use super::{Allows, Handled, Handles};
use crate::Actor;
use aethereal_data::Kind;

/// An actor of this type takes every kind of the protocol `P`, so an [`ActorRef`](crate::ActorRef) to one converts
/// into a [`ProtocolRef<P>`](crate::ProtocolRef) (ADR-0016).
///
/// For a tuple of kinds, it conforms when it handles each of them. The compiler checks it, and nothing is read when
/// the program runs.
pub trait Conforms<P> {}

/// The protocol takes every kind of the tuple of kinds `Q`, each found at its place in `Is`, which the compiler infers:
/// a [`ProtocolRef`](crate::ProtocolRef) to it narrows to one to `Q` (ADR-0016).
///
/// The kinds may be in any order, so `(A, B)` covers `(B, A)`. A kind found twice is not inferred, and does not
/// build.
pub trait Covers<Q, Is> {}

impl<T> Conforms<()> for T {}

impl<P> Covers<(), ()> for P {}

macro_rules! protocols {
    ($( [$($kind:ident $is:ident)*] )*) => {$(
        impl<T: Actor, $($kind: Kind + Handled),*> Conforms<($($kind,)*)> for T where $(T: Handles<$kind>),* {}

        impl<P, $($kind, $is),*> Covers<($($kind,)*), ($($is,)*)> for P where $(P: Allows<$kind, $is>),* {}
    )*};
}

protocols! {
    [A IA]
    [A IA B IB]
    [A IA B IB C IC]
    [A IA B IB C IC D ID]
    [A IA B IB C IC D ID E IE]
    [A IA B IB C IC D ID E IE F IF]
    [A IA B IB C IC D ID E IE F IF G IG]
    [A IA B IB C IC D ID E IE F IF G IG H IH]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK]
    [A IA B IB C IC D ID E IE F IF G IG H IH I II J IJ K IK L IL]
}
