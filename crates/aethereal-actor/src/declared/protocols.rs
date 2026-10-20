//! What a reference typed by a protocol is proven to reach: the protocols, a type that conforms to one, and a protocol
//! that covers another (ADR-0016).

use super::{Allows, Handled, Handles, Kinds, RepliesWithin};
use crate::Actor;
use aethereal_data::Kind;

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
pub trait CarriedBy<P> {}

/// A protocol a [`ProtocolRef`](crate::ProtocolRef) reaches an actor by: a tuple of kinds, which are all tells, or a
/// named [`Protocol`] (ADR-0016).
pub trait Listed {
    /// Its kinds, as a tuple.
    type Kinds: Kinds;
}

impl<P: Protocol> Listed for P {
    type Kinds = P::Kinds;
}

impl Listed for () {
    type Kinds = ();
}

impl<P> CarriedBy<P> for () {}

/// An actor of this type takes every kind of the protocol `P`, each found at its place in `Is`, which the compiler
/// infers: an [`ActorRef`](crate::ActorRef) to one is made a [`ProtocolRef<P>`](crate::ProtocolRef) by its
/// `protocol` (ADR-0016).
///
/// It conforms when it handles each kind of `P`. For a named protocol, each
/// reply its handler of a kind declares must also be of a kind the protocol
/// answers that kind with, so a sender through the protocol, which handles
/// those, handles whatever comes back. A handler need not send every answer.
/// The compiler checks it, and nothing is read when the program runs.
///
/// A board whose handler replies with a kind the protocol does not answer with does not conform:
///
/// ```compile_fail
/// # use aethereal_actor::{Actor, ActorRef, Beginning, Carries, Ending, Fault, Handles, Protocol, ProtocolRef, ToSender, Turn};
/// # use aethereal_data::{Kind, KindName, Plain};
/// #
/// # #[derive(Plain)]
/// # struct WhichTile;
/// #
/// # impl Kind for WhichTile {
/// #     const NAME: KindName = KindName::new("aether.example.which_tile");
/// # }
/// #
/// # #[derive(Plain)]
/// # struct TileIs;
/// #
/// # impl Kind for TileIs {
/// #     const NAME: KindName = KindName::new("aether.example.tile_is");
/// # }
/// #
/// # #[derive(Plain)]
/// # struct Lost;
/// #
/// # impl Kind for Lost {
/// #     const NAME: KindName = KindName::new("aether.example.lost");
/// # }
/// #
/// # struct Tiles;
/// #
/// # impl Protocol for Tiles {
/// #     type Kinds = (WhichTile,);
/// # }
/// #
/// # impl Carries<WhichTile> for Tiles {
/// #     type Answers = (TileIs,);
/// # }
/// #
/// # #[derive(Plain)]
/// # struct Board;
/// #
/// # impl Kind for Board {
/// #     const NAME: KindName = KindName::new("aether.example.board");
/// # }
/// #
/// # impl Actor for Board {
/// #     type Config = Board;
/// #     type Key = ();
/// #     type Handles = (WhichTile,);
/// #     type BeginSends = ();
/// #     type EndSends = ();
/// #
/// #     fn begin(board: Board, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
/// #         Ok(board)
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// /// Answers which tile, or that it is lost: a reply the protocol does not answer with.
/// impl Handles<WhichTile> for Board {
///     type Sends = (ToSender<TileIs>, ToSender<Lost>);
///
///     fn handle(&mut self, _: WhichTile, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
///         Ok(turn.reply(&Lost)?)
///     }
/// }
///
/// fn tiles(board: ActorRef<Board>) -> ProtocolRef<Tiles> {
///     board.protocol()
/// }
/// ```
pub trait Conforms<P, Is> {}

/// The actor of type `T` handles each of these kinds of the named protocol `P`, and replies to each only with what
/// `P` answers it with, each reply found at its place in `Is`.
pub trait AnsweredBy<T, P, Is> {}

impl<T: Actor, P: Protocol, Is> Conforms<P, Is> for T where P::Kinds: AnsweredBy<T, P, Is> {}

/// The protocol takes every kind of the tuple of kinds `Q`, each found at its place in `Is`, which the compiler infers:
/// a [`ProtocolRef`](crate::ProtocolRef) to it narrows to one to `Q` (ADR-0016).
///
/// The kinds may be in any order, so `(A, B)` covers `(B, A)`. A kind found twice is not inferred, and does not
/// build.
pub trait Covers<Q, Is> {}

impl<T> Conforms<(), ()> for T {}

impl<T, P> AnsweredBy<T, P, ()> for () {}

impl<P> Covers<(), ()> for P {}

macro_rules! protocols {
    ($( [$($kind:ident $is:ident)*] )*) => {$(
        impl<T: Actor, $($kind: Kind + Handled),*> Conforms<($($kind,)*), ()> for T where $(T: Handles<$kind>),* {}

        impl<T: Actor, P, $($kind: Kind + Handled, $is),*> AnsweredBy<T, P, ($($is,)*)> for ($($kind,)*)
        where
            $(T: Handles<$kind>, P: Carries<$kind>, <T as Handles<$kind>>::Sends: RepliesWithin<<P as Carries<$kind>>::Answers, $is>,)*
        {
        }

        impl<P, $($kind, $is),*> Covers<($($kind,)*), ($($is,)*)> for P where $(P: Allows<$kind, $is>),* {}

        impl<$($kind: Kind),*> Listed for ($($kind,)*) {
            type Kinds = Self;
        }

        impl<P: $(Carries<$kind> +)*, $($kind: Kind),*> CarriedBy<P> for ($($kind,)*) {}
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
