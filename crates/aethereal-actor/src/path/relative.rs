//! Places named from where the sender stands: its parent, by what it takes, or a child beneath it, by its type
//! (ADR-0016, ADR-0022).

use crate::Vantage;
use crate::{
    Actor, Admits, Anywhere, ChildOf, Handled, Handles, Listed, Lists, PlacesBeneath, Reachable, Reaches, Seated,
};
use aethereal_data::{Kind, KindId, PathSegment, Then};
use alloc::vec::Vec;
use core::marker::PhantomData;

/// The actor directly above the sender, when its type lists every kind of the protocol `P`: what [`parent`] names.
pub struct Parent<P> {
    of: PhantomData<fn() -> P>,
}

/// The actor directly above the sender, as the target of its sends, when its type lists every kind of the protocol
/// `P`, as a [`cast`](crate::Turn::cast) to `P` finds. It builds only for a type that may stand [`Anywhere`], whose
/// parent's type is a fact of the run, so what it takes is what is named; a type that stands beneath one type, or its
/// own, sends through the reference [`Turn::parent`](crate::Turn::parent) answers. Like a typed path, it is found
/// when the target is made, and a parent that does not list `P`, or none, is as an empty place: mail to it is
/// dropped.
///
/// ```compile_fail,E0277
/// # use aethereal_actor::{Fault, Turn, actor, parent};
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.doc.poke")]
/// # struct Poke;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.doc.army")]
/// # struct Army;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.doc.squad")]
/// # struct Squad;
/// #
/// # #[actor]
/// # impl Army {
/// #     fn begin(army: Army) -> Result<Self, Fault> {
/// #         Ok(army)
/// #     }
/// #
/// #     #[tell]
/// #     fn poke(&mut self, _: Poke) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// // A squad stands beneath one type: it sends to its parent through `turn.parent()`.
/// #[actor(parent = Army)]
/// impl Squad {
/// #     fn begin(squad: Squad) -> Result<Self, Fault> {
/// #         Ok(squad)
/// #     }
/// #
///     #[tell]
///     fn poke(&mut self, _: Poke, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
///         turn.to(parent()).send(&Poke)?;
///
///         Ok(())
///     }
/// }
/// ```
#[must_use]
pub const fn parent<P>() -> Parent<P> {
    Parent { of: PhantomData }
}

impl<Me: ChildOf<Parent = Anywhere>, P: Listed> Reachable<Me> for Parent<P> {
    type Through = Self;
    type At = Seated;

    fn at(self, from: &Vantage<'_, '_>) -> Seated {
        from.parent::<P::Kinds>()
    }
}

/// A parent is sent to as an actor reached through a protocol reference is: of a kind the protocol holds.
impl<Me, P: Listed, K: Kind, I> Reaches<Me, K, I> for Parent<P> where P::Kinds: Lists<K, I> {}

/// The place beneath the sender that the types `D` name, from a child of its own down: what [`child`] names.
pub struct Child<D> {
    /// The segments above the last, from the child of the sender's own down: none for a child of its own.
    above: Vec<PathSegment>,
    /// The segment of the actor the place holds.
    last: PathSegment,
    of: PhantomData<fn() -> D>,
}

impl<C: ChildOf> Child<C> {
    /// The segment of the child of the sender's own this names: what a receiver finds it by, one segment beneath the
    /// sender, as the renderer finds a child a draw list calls (ADR-0055).
    #[must_use]
    pub fn into_segment(self) -> PathSegment {
        self.last
    }
}

/// The types of a place beneath the sender, from a child of its own down to the last: one type, or a tuple of up to
/// eight, each of which may stand beneath the one before it, checked as the program is built. A tuple restates no
/// `Parent`, as the types between the sender and the last are not found from it: a type may stand beneath several.
pub trait Descent {
    /// The type that stands directly beneath the sender.
    type First: ChildOf;
    /// The type of the actor the place holds.
    type Last: ChildOf;
    /// The keys of the keyed segments, from the first: `()` for none, a key alone for one, a tuple for several, as
    /// for a typed path.
    type Keys;

    /// Puts the segments above the place's last, from the first, after those in `above`, and answers the last.
    fn place(keys: Self::Keys, above: &mut Vec<PathSegment>) -> PathSegment;
}

impl<C: ChildOf> Descent for C
where
    (): Then<C::Key>,
{
    type First = C;
    type Last = C;
    type Keys = <() as Then<C::Key>>::Keys;

    fn place(keys: Self::Keys, _: &mut Vec<PathSegment>) -> PathSegment {
        let ((), key) = <() as Then<C::Key>>::split(keys);

        PathSegment::new(C::NAME, &key)
    }
}

/// A tuple of types is the place its types but the last name, `$above`, then the last beneath the one before it.
macro_rules! descent {
    ($above:ty; $($each:ident),+; $before:ident, $last:ident) => {
        impl<$($each),+> Descent for ($($each),+)
        where
            $above: Descent<Last = $before>,
            $before: ChildOf,
            $last: ChildOf,
            $last::Parent: PlacesBeneath<$before, $last>,
            <$above as Descent>::Keys: Then<$last::Key>,
        {
            type First = <$above as Descent>::First;
            type Last = $last;
            type Keys = <<$above as Descent>::Keys as Then<$last::Key>>::Keys;

            fn place(keys: Self::Keys, above: &mut Vec<PathSegment>) -> PathSegment {
                let (keys, key) = <<$above as Descent>::Keys as Then<$last::Key>>::split(keys);
                let before = <$above as Descent>::place(keys, above);
                above.push(before);

                PathSegment::new($last::NAME, &key)
            }
        }
    };
}

descent!(A; A, B; A, B);
descent!((A, B); A, B, C; B, C);
descent!((A, B, C); A, B, C, D; C, D);
descent!((A, B, C, D); A, B, C, D, E; D, E);
descent!((A, B, C, D, E); A, B, C, D, E, F; E, F);
descent!((A, B, C, D, E, F); A, B, C, D, E, F, G; F, G);
descent!((A, B, C, D, E, F, G); A, B, C, D, E, F, G, H; G, H);

/// The place beneath the sender that the types `D` name under `keys`, as the target of its sends: a child of its
/// own, `child::<Unit>(7)`, or a place beneath one, `child::<(Unit, Squad)>((7, 2))`. It builds only when the first
/// type may stand beneath the sender's, and each after it beneath the one before ([`Descent`]); its keys are as a
/// typed path's. Like a typed path, it is found when the target is made, and a place that holds none of its last type
/// is empty: mail to it is dropped. A child of the sender's own is found among those it made, in this step or before,
/// live or not; a place beneath one as a typed path is, live in the table as the step began.
///
/// ```compile_fail,E0277
/// # use aethereal_actor::{Fault, Turn, actor, child};
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.doc.poke")]
/// # struct Poke;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.doc.army")]
/// # struct Army;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.doc.squad")]
/// # struct Squad;
/// #
/// # #[actor]
/// # impl Army {
/// #     fn begin(army: Army) -> Result<Self, Fault> {
/// #         Ok(army)
/// #     }
/// #
/// #     #[tell]
/// #     fn poke(&mut self, _: Poke) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// // A squad stands beneath an army, and nothing else.
/// #[actor(parent = Army)]
/// impl Squad {
/// #     fn begin(squad: Squad) -> Result<Self, Fault> {
/// #         Ok(squad)
/// #     }
/// #
///     #[tell]
///     fn poke(&mut self, _: Poke, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
///         turn.to(child::<Squad>(())).send(&Poke)?;
///
///         Ok(())
///     }
/// }
/// ```
///
/// Nor does a place whose types do not stand each beneath the one before:
///
/// ```compile_fail,E0277
/// # use aethereal_actor::{Fault, Turn, actor, child};
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.doc.poke")]
/// # struct Poke;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.doc.army")]
/// # struct Army;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.doc.squad")]
/// # struct Squad;
/// #
/// // A squad stands beneath an army, so no squad stands beneath the army's squad.
/// #[actor]
/// impl Army {
/// #     fn begin(army: Army) -> Result<Self, Fault> {
/// #         Ok(army)
/// #     }
/// #
///     #[tell]
///     fn poke(&mut self, _: Poke, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
///         turn.to(child::<(Squad, Squad)>(())).send(&Poke)?;
///
///         Ok(())
///     }
/// }
/// #
/// # #[actor(parent = Army)]
/// # impl Squad {
/// #     fn begin(squad: Squad) -> Result<Self, Fault> {
/// #         Ok(squad)
/// #     }
/// #
/// #     #[tell]
/// #     fn poke(&mut self, _: Poke) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// ```
#[must_use]
pub fn child<D: Descent>(keys: D::Keys) -> Child<D> {
    let mut above = Vec::new();
    let last = D::place(keys, &mut above);

    Child { above, last, of: PhantomData }
}

impl<Me, D: Descent> Reachable<Me> for Child<D>
where
    <D::First as ChildOf>::Parent: PlacesBeneath<Me, D::First>,
{
    type Through = Self;
    type At = Seated;

    /// A child of the sender's own is found among those it made; a place beneath one, as a typed path is found.
    fn at(self, from: &Vantage<'_, '_>) -> Seated {
        let kind = KindId::of::<D::Last>();
        match self.above.as_slice() {
            [] => from.child(&self.last, kind),
            above => from.beneath(above, &self.last, kind),
        }
    }
}

/// A place is sent to as an actor of its last type is: of a kind that type handles, from a sender its guard admits.
impl<Me, D, K, I> Reaches<Me, K, I> for Child<D>
where
    D: Descent,
    D::Last: Actor + Handles<K>,
    <D::Last as Handles<K>>::From: Admits<D::Last, Me, I>,
    K: Kind + Handled,
{
}
