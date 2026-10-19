//! What a reference typed by a protocol is proven to reach: the protocols, a type that conforms to one, and a protocol
//! that covers another (ADR-0016).

use super::{Allows, Handled, Handles, Kinds};
use crate::Actor;
use aethereal_data::{Kind, KindId};

/// A named protocol: a list of the kinds it takes, and what each is answered with, one [`Carries`] for each, as an
/// actor lists what it handles and writes one handler for each (ADR-0016).
///
/// ```
/// use aethereal_actor::{Carries, Protocol};
/// use aethereal_data::{Kind, KindName, Plain};
///
/// #[derive(Plain)]
/// struct WhichTile;
///
/// impl Kind for WhichTile {
///     const NAME: KindName = KindName::new("aether.example.which_tile");
/// }
///
/// #[derive(Plain)]
/// struct TileIs;
///
/// impl Kind for TileIs {
///     const NAME: KindName = KindName::new("aether.example.tile_is");
/// }
///
/// struct Tiles;
///
/// impl Protocol for Tiles {
///     type Kinds = (WhichTile,);
/// }
///
/// impl Carries<WhichTile> for Tiles {
///     type Answers = (TileIs,);
/// }
/// ```
///
/// A kind it lists with no [`Carries`] does not build:
///
/// ```compile_fail
/// use aethereal_actor::Protocol;
/// use aethereal_data::{Kind, KindName, Plain};
///
/// #[derive(Plain)]
/// struct Highlight;
///
/// impl Kind for Highlight {
///     const NAME: KindName = KindName::new("aether.example.highlight");
/// }
///
/// struct Tiles;
///
/// impl Protocol for Tiles {
///     type Kinds = (Highlight,);
/// }
/// ```
pub trait Protocol: Sized {
    /// The kinds it takes: a tuple of kinds.
    type Kinds: Kinds + CarriedBy<Self>;
}

/// What the named protocol answers mail of kind `K` with: `Answers`, a tuple of kinds, `()` for a tell (ADR-0016).
pub trait Carries<K: Kind> {
    /// The kinds the receiver sends back to the sender.
    type Answers: Kinds;
}

/// The protocol `P` has a [`Carries`] for each kind of this tuple: what makes a named protocol's list its own.
pub trait CarriedBy<P> {
    /// What each kind is answered with, in the tuple's order.
    const ANSWERS: &'static [Answering];
}

/// What mail of one kind of a protocol is answered with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Answering {
    /// The kind of the mail.
    pub kind: KindId,
    /// The kinds it is answered with, sent back to its sender.
    pub answers: &'static [KindId],
}

/// A protocol a [`ProtocolRef`](crate::ProtocolRef) reaches an actor by: a tuple of kinds, which are all tells, or a
/// named [`Protocol`] (ADR-0016).
pub trait Listed {
    /// Its kinds, as a tuple.
    type Kinds: Kinds;
    /// What each of its kinds is answered with: nothing, for a tuple.
    const ANSWERS: &'static [Answering];
}

impl<P: Protocol> Listed for P {
    type Kinds = P::Kinds;
    const ANSWERS: &'static [Answering] = <P::Kinds as CarriedBy<P>>::ANSWERS;
}

impl Listed for () {
    type Kinds = ();
    const ANSWERS: &'static [Answering] = &[];
}

impl<P> CarriedBy<P> for () {
    const ANSWERS: &'static [Answering] = &[];
}

/// An actor of this type takes every kind of the protocol `P`, so an [`ActorRef`](crate::ActorRef) to one converts
/// into a [`ProtocolRef<P>`](crate::ProtocolRef) (ADR-0016).
///
/// It conforms when it handles each kind of `P`. For a named protocol, its
/// handler of each kind must also declare a reply of each kind the protocol
/// answers it with. The conversion checks that when the program is built, so
/// a conversion nothing builds is not checked, as a declaration is not
/// (ADR-0013). Nothing is read when the program runs.
///
/// A board that takes the question a protocol answers, and declares no answer, does not convert:
///
/// ```compile_fail
/// use aethereal_actor::{Actor, ActorRef, Beginning, Carries, Ending, Fault, Handles, Protocol, ProtocolRef, Turn};
/// use aethereal_data::{Kind, KindName, Plain};
///
/// #[derive(Plain)]
/// struct WhichTile;
///
/// impl Kind for WhichTile {
///     const NAME: KindName = KindName::new("aether.example.which_tile");
/// }
///
/// #[derive(Plain)]
/// struct TileIs;
///
/// impl Kind for TileIs {
///     const NAME: KindName = KindName::new("aether.example.tile_is");
/// }
///
/// struct Tiles;
///
/// impl Protocol for Tiles {
///     type Kinds = (WhichTile,);
/// }
///
/// impl Carries<WhichTile> for Tiles {
///     type Answers = (TileIs,);
/// }
///
/// /// A board that takes the question, and never answers it.
/// #[derive(Plain)]
/// struct Board;
///
/// impl Kind for Board {
///     const NAME: KindName = KindName::new("aether.example.board");
/// }
///
/// impl Actor for Board {
///     type Config = Board;
///     type Key = ();
///     type Handles = (WhichTile,);
///     type BeginSends = ();
///     type EndSends = ();
///
///     fn begin(board: Board, _: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
///         Ok(board)
///     }
///
///     fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// impl Handles<WhichTile> for Board {
///     type Sends = ();
///
///     fn handle(&mut self, _: WhichTile, _: &mut Turn<'_, ()>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// fn tiles(board: ActorRef<Board>) -> ProtocolRef<Tiles> {
///     board.into()
/// }
/// fn main() {
///     let _ = tiles as fn(ActorRef<Board>) -> ProtocolRef<Tiles>;
/// }
/// ```
pub trait Conforms<P> {}

impl<T: Actor + Conforms<P::Kinds>, P: Protocol> Conforms<P> for T {}

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

        impl<$($kind: Kind),*> Listed for ($($kind,)*) {
            type Kinds = Self;
            const ANSWERS: &'static [Answering] = &[];
        }

        impl<P: $(Carries<$kind> +)*, $($kind: Kind),*> CarriedBy<P> for ($($kind,)*) {
            const ANSWERS: &'static [Answering] = &[
                $(Answering { kind: KindId::of::<$kind>(), answers: <P as Carries<$kind>>::Answers::KINDS }),*
            ];
        }
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
