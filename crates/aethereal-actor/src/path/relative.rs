//! Places named from where the sender stands: its parent, by what it takes, or a child beneath it, by its type
//! (ADR-0016, ADR-0022).

use crate::Vantage;
use crate::{Actor, Admits, Anywhere, ChildOf, Handled, Handles, Listed, Reachable, Reaches, Seated, ToAny, ToType};
use aethereal_data::{Kind, KindId, PathSegment};
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
/// # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Handles, ToAny, ToType, Top, Turn, child, parent};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # macro_rules! actor {
/// #     ($name:ident, $named:literal, $parent:ty) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new($named);
/// #         }
/// #
/// #         impl ChildOf for $name {
/// #             type Parent = $parent;
/// #         }
/// #     };
/// # }
/// # #[derive(Plain)]
/// # struct Poke;
/// #
/// # impl Kind for Poke {
/// #     const NAME: KindName = KindName::new("aether.doc.poke");
/// # }
/// #
/// # impl Actor for Squad {
/// #     type Config = Squad;
/// #     type Key = ();
/// #     type Handles = (Poke,);
/// #     type BeginSends = ();
/// #     type EndSends = ();
/// #
/// #     fn begin(squad: Squad, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
/// #         Ok(squad)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// # impl Actor for Army {
/// #     type Config = Army;
/// #     type Key = ();
/// #     type Handles = (Poke,);
/// #     type BeginSends = ();
/// #     type EndSends = ();
/// #
/// #     fn begin(army: Army, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
/// #         Ok(army)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// actor!(Army, "aether.doc.army", Top);
/// // A squad stands beneath one type: it sends to its parent through `turn.parent()`.
/// actor!(Squad, "aether.doc.squad", Army);
///
/// impl Handles<Poke> for Squad {
///     type Sends = (ToAny<(Poke,), Poke>,);
///
///     fn handle(&mut self, _: Poke, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
///         turn.to(parent()).send(&Poke)?;
///
///         Ok(())
///     }
/// }
/// #
/// # impl Handles<Poke> for Army {
/// #     type Sends = ();
/// #
/// #     fn handle(&mut self, _: Poke, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
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

/// A parent is sent to as an actor reached through a protocol reference is: the sends list [`ToAny<P, K>`].
impl<P: Listed, K: Kind> Reaches<K> for Parent<P> {
    type Marker = ToAny<P, K>;
}

/// The child of type `C` beneath the sender under a key: what [`child`] names.
pub struct Child<C> {
    segment: PathSegment,
    of: PhantomData<fn() -> C>,
}

/// The child of type `C` under `key` beneath the sender, as the target of its sends. It builds only when `C` may stand
/// beneath the sender's type. Like a typed path, it is found when the target is made, among the children
/// the sender made, and a place that holds none of its type is empty: mail to it is dropped.
///
/// ```compile_fail,E0277
/// # use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Handles, ToAny, ToType, Top, Turn, child, parent};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # macro_rules! actor {
/// #     ($name:ident, $named:literal, $parent:ty) => {
/// #         #[derive(Plain)]
/// #         struct $name;
/// #
/// #         impl Kind for $name {
/// #             const NAME: KindName = KindName::new($named);
/// #         }
/// #
/// #         impl ChildOf for $name {
/// #             type Parent = $parent;
/// #         }
/// #     };
/// # }
/// # #[derive(Plain)]
/// # struct Poke;
/// #
/// # impl Kind for Poke {
/// #     const NAME: KindName = KindName::new("aether.doc.poke");
/// # }
/// #
/// # impl Actor for Squad {
/// #     type Config = Squad;
/// #     type Key = ();
/// #     type Handles = (Poke,);
/// #     type BeginSends = ();
/// #     type EndSends = ();
/// #
/// #     fn begin(squad: Squad, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
/// #         Ok(squad)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// # impl Actor for Army {
/// #     type Config = Army;
/// #     type Key = ();
/// #     type Handles = (Poke,);
/// #     type BeginSends = ();
/// #     type EndSends = ();
/// #
/// #     fn begin(army: Army, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
/// #         Ok(army)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// actor!(Army, "aether.doc.army", Top);
/// // A squad stands beneath an army, and nothing else.
/// actor!(Squad, "aether.doc.squad", Army);
///
/// impl Handles<Poke> for Squad {
///     type Sends = (ToType<Squad, Poke>,);
///
///     fn handle(&mut self, _: Poke, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
///         turn.to(child::<Squad>(&())).send(&Poke)?;
///
///         Ok(())
///     }
/// }
/// #
/// # impl Handles<Poke> for Army {
/// #     type Sends = ();
/// #
/// #     fn handle(&mut self, _: Poke, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// ```
#[must_use]
pub fn child<C: ChildOf>(key: &C::Key) -> Child<C> {
    Child { segment: PathSegment::new(C::NAME, key), of: PhantomData }
}

impl<Me, C: ChildOf> Reachable<Me> for Child<C>
where
    C::Parent: Admits<Me, C>,
{
    type Through = Self;
    type At = Seated;

    fn at(self, from: &Vantage<'_, '_>) -> Seated {
        from.child(&self.segment, KindId::of::<C>())
    }
}

/// A child is sent to as an actor of its type is: the sends list [`ToType<C, K>`].
impl<C: Actor + Handles<K>, K: Kind + Handled> Reaches<K> for Child<C> {
    type Marker = ToType<C, K>;
}
