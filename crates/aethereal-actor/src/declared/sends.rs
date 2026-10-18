//! What a handler may send, and whether it may watch, written as a tuple of markers (ADR-0013).

use super::{Handled, Handles, Sending, Target};
use crate::Actor;
use aethereal_data::{Kind, KindId, KindName, Plain};
use core::marker::PhantomData;

/// A handler may send mail of kind `K` to an actor of type `T`, which handles it.
pub struct ToType<T, K>(PhantomData<fn() -> (T, K)>);

/// A handler may send mail of kind `K` to the source of the item it handles.
pub struct ToSender<K>(PhantomData<fn() -> K>);

/// A handler may send mail of kind `K` to any actor whose type lists every kind of `P`, a tuple of kinds: the
/// actors a reference that names a protocol, and not a type, reaches.
pub struct ToAny<P, K>(PhantomData<fn() -> (P, K)>);

/// A handler may watch an actor, to be handed its ending: only an actor whose type handles
/// [`Ended`](crate::Ended) may declare it, as its build fails otherwise (ADR-0013).
pub struct Watches;

/// What a watch is declared under: a kind of its own, which no mail is.
#[derive(Plain)]
struct Watch;

impl Kind for Watch {
    const NAME: KindName = KindName::new("aether.actor.watches");
}

/// One thing a handler declares it may do: a send, the kind it sends and where to, or a watch (ADR-0013).
pub trait Marker {
    /// What it declares, as the engine reads it.
    const SENDING: Sending;
}

impl Marker for Watches {
    const SENDING: Sending = Sending::new(KindId::of::<Watch>(), Target::Watched);
}

impl<T: Actor + Handles<K>, K: Kind + Handled> Marker for ToType<T, K> {
    const SENDING: Sending = Sending::new(KindId::of::<K>(), Target::Type(Name::<T>::NAME, KindId::of::<T>()));
}

impl<K: Kind> Marker for ToSender<K> {
    const SENDING: Sending = Sending::new(KindId::of::<K>(), Target::Sender);
}

impl<P: Kinds, K: Kind> Marker for ToAny<P, K> {
    const SENDING: Sending = Sending::new(KindId::of::<K>(), Target::Handling(P::KINDS));
}

/// `T`'s name held by reference, so a constant can read it without dropping it.
struct Name<T>(PhantomData<T>);

impl<T: Kind> Name<T> {
    const NAME: &'static KindName = &T::NAME;
}

/// Everything a handler may send: a tuple of [`Marker`]s, `()` for nothing (ADR-0013).
pub trait Sends {
    /// The sends, as the engine reads them.
    const SENDINGS: &'static [Sending];
}

/// A tuple of kinds: what [`ToAny`] names a protocol by.
pub trait Kinds {
    /// The ids of the kinds, in order.
    const KINDS: &'static [KindId];
}

/// The list of sends allows the send `M`, at `I`, its place in the tuple, which the compiler infers.
///
/// A send whose marker is not in its handler's list does not build.
pub trait Allows<M, I> {}

/// A place in a tuple of markers.
pub struct At<const PLACE: usize>;

impl Sends for () {
    const SENDINGS: &'static [Sending] = &[];
}

impl Kinds for () {
    const KINDS: &'static [KindId] = &[];
}

macro_rules! tuples {
    ($( [$($name:ident $place:literal)*] )*) => {$(
        impl<$($name: Marker),*> Sends for ($($name,)*) {
            const SENDINGS: &'static [Sending] = &[$($name::SENDING),*];
        }

        impl<$($name: Kind),*> Kinds for ($($name,)*) {
            const KINDS: &'static [KindId] = &[$(KindId::of::<$name>()),*];
        }

        tuples!(@allows [$($name)*] $($name $place)*);
    )*};
    (@allows $all:tt $($name:ident $place:literal)*) => {$(
        tuples!(@allow $all $name $place);
    )*};
    (@allow [$($all:ident)*] $name:ident $place:literal) => {
        impl<$($all),*> Allows<$name, At<$place>> for ($($all,)*) {}
    };
}

tuples! {
    [A 0]
    [A 0 B 1]
    [A 0 B 1 C 2]
    [A 0 B 1 C 2 D 3]
    [A 0 B 1 C 2 D 3 E 4]
    [A 0 B 1 C 2 D 3 E 4 F 5]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8 J 9]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8 J 9 K 10]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8 J 9 K 10 L 11]
}
