//! A list of actor types, as an actor's `Depends` and `Spawns` hold one: one type, `()`, a tuple of lists, nested, or
//! a list type parameter, [`Each<L>`], as the types it lists (ADR-0059).

use crate::ChildOf;
use core::marker::PhantomData;

/// The types the list type parameter `L` lists, as an entry of an actor's `Depends` or `Spawns`: what a generic actor
/// written over a list of types, as a toolbox is over its tools, depends on or spawns (ADR-0059).
///
/// A type is found in it [`Through`] a path into `L`, [`Pick`], which the generic code that reaches the type carries,
/// as nothing there names the types `L` lists. The program's build check walks it as the types it lists, as the
/// program names `L`.
pub struct Each<L>(PhantomData<fn() -> L>);

/// A list of actor types (ADR-0059): an actor type, a list of itself, of the form [`One`]; or `()`, a tuple of up to
/// 16 lists, or [`Each<L>`], of the form [`Many`]. What walks a list reads its form, so each step of the walk has one
/// way to go, and a list a program cannot fit is named where it fails.
pub trait TypeList {
    /// The list's form: [`One`] or [`Many`].
    type Form;
}

/// The form of an actor type as a list: a list of itself.
pub enum One {}

/// The form of `()`, a tuple of lists, and [`Each<L>`]: a list of the types its entries list.
pub enum Many {}

impl<T: ChildOf> TypeList for T {
    type Form = One;
}

impl TypeList for () {
    type Form = Many;
}

impl<L: TypeList> TypeList for Each<L> {
    type Form = Many;
}

/// A path into the list `S` picks [`Picked`](Self::Picked), a list within it: [`Whole`], or [`Down`] to an entry of
/// what a shorter path picks. Generic code that walks a list type parameter carries the path to where it stands, and
/// the next step's path is found from it, as that code cannot name the types it reaches (ADR-0059).
pub trait Pick<S> {
    /// The list the path picks.
    type Picked;
}

/// The path that picks the whole list.
pub struct Whole;

/// The path that goes down to the entry at `N` of the tuple the path `P` picks.
pub struct Down<P, const N: usize>(PhantomData<fn() -> P>);

impl<S> Pick<S> for Whole {
    type Picked = S;
}

impl<S, P: Pick<S>, const N: usize> Pick<S> for Down<P, N>
where
    P::Picked: Nth<N>,
{
    type Picked = <P::Picked as Nth<N>>::Of;
}

/// A tuple's entry at `N`.
pub trait Nth<const N: usize> {
    /// The entry.
    type Of;
}

/// A type found in an [`Each<L>`] through the path `P` into `L`, which the generic code that finds it states
/// (ADR-0059).
pub struct Through<P>(PhantomData<fn() -> P>);

/// The most entries a list holds as the engine reads it, its nested lists' counted: a constant has no room to grow.
const MOST: usize = 64;

/// The entries of several lists, one after another, as a constant holds them: what the engine reads a tuple of lists
/// as.
pub(super) enum Joined<T: 'static> {
    /// The lists hold nothing.
    Empty,
    /// The first `len` of `entries`, the room past them filled with the first.
    Filled { entries: [T; MOST], len: usize },
}

impl<T: Copy> Joined<T> {
    /// The entries of `lists`, in order.
    ///
    /// # Panics
    ///
    /// Panics, stopping the build, when they are more than 64.
    pub(super) const fn of(lists: &[&'static [T]]) -> Self {
        let mut list = 0;
        while list < lists.len() && lists[list].is_empty() {
            list += 1;
        }
        if list == lists.len() {
            return Self::Empty;
        }
        let mut entries = [lists[list][0]; MOST];
        let mut len = 0;
        while list < lists.len() {
            let mut at = 0;
            while at < lists[list].len() {
                assert!(len < MOST, "a list holds at most 64 types, its nested lists' counted");
                entries[len] = lists[list][at];
                len += 1;
                at += 1;
            }
            list += 1;
        }

        Self::Filled { entries, len }
    }

    /// The entries.
    pub(super) const fn entries(&'static self) -> &'static [T] {
        match self {
            Self::Empty => &[],
            Self::Filled { entries, len } => entries.split_at(*len).0,
        }
    }
}

/// What the engine reads of the tuple of lists `L`, joined once for each tuple type.
pub(super) struct Tuple<L>(PhantomData<fn() -> L>);

macro_rules! tuples {
    ($( [$($name:ident $place:literal)*] )*) => {$(
        impl<$($name: TypeList),*> TypeList for ($($name,)*) {
            type Form = Many;
        }

        tuples!(@entries [$($name)*] $($name $place)*);
    )*};
    (@entries $all:tt $($name:ident $place:literal)*) => {$(
        tuples!(@entry $all $name $place);
    )*};
    (@entry [$($all:ident)*] $name:ident $place:literal) => {
        impl<$($all),*> Nth<$place> for ($($all,)*) {
            type Of = $name;
        }
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
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8 J 9 K 10 N 11]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8 J 9 K 10 N 11 O 12]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8 J 9 K 10 N 11 O 12 P 13]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8 J 9 K 10 N 11 O 12 P 13 Q 14]
    [A 0 B 1 C 2 D 3 E 4 F 5 G 6 H 7 I 8 J 9 K 10 N 11 O 12 P 13 Q 14 S 15]
}
